// SPDX-License-Identifier: GPL-3.0-or-later
//! Durable stop requests shared by console signals, systemd and the Windows SCM.

use std::io;
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct Control {
    sender: watch::Sender<bool>,
    #[cfg(windows)]
    native: bool,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            sender: watch::channel(false).0,
            #[cfg(windows)]
            native: false,
        }
    }
}

impl Control {
    #[cfg(windows)]
    pub(crate) fn native() -> Self {
        Self {
            native: true,
            ..Self::default()
        }
    }

    pub(crate) fn request(&self) {
        self.sender.send_replace(true);
    }
    pub(crate) fn requested(&self) -> bool {
        *self.sender.borrow()
    }
    pub(crate) fn receiver(&self) -> watch::Receiver<bool> {
        self.sender.subscribe()
    }
}

pub(crate) async fn wait(mut receiver: watch::Receiver<bool>) {
    // A request is durable even if it arrived before subscription or readiness.
    drop(receiver.wait_for(|stopping| *stopping).await);
}

pub(crate) struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    console: Option<(
        tokio::signal::windows::CtrlC,
        tokio::signal::windows::CtrlBreak,
    )>,
}

impl Signals {
    pub(crate) fn register(
        #[cfg(windows)] control: &Control,
        #[cfg(not(windows))] _: &Control,
    ) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                console: if control.native {
                    None
                } else {
                    Some((
                        tokio::signal::windows::ctrl_c()?,
                        tokio::signal::windows::ctrl_break()?,
                    ))
                },
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(Self {})
        }
    }

    pub(crate) async fn wait(&mut self, control: &Control) {
        #[cfg(unix)]
        tokio::select! { _ = self.interrupt.recv() => {}, _ = self.terminate.recv() => {}, () = wait(control.receiver()) => {} }
        #[cfg(windows)]
        if let Some((interrupt, cancel)) = &mut self.console {
            tokio::select! { _ = interrupt.recv() => {}, _ = cancel.recv() => {}, () = wait(control.receiver()) => {} }
        } else {
            wait(control.receiver()).await;
        }
        #[cfg(not(any(unix, windows)))]
        wait(control.receiver()).await;
        control.request();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stop_is_not_lost_before_a_receiver_subscribes() {
        let control = Control::default();
        control.request();
        assert!(control.requested());
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            wait(control.receiver()),
        )
        .await
        .unwrap();
    }
}

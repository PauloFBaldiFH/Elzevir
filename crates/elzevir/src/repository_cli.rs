// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline pilot. The installation lock covers both SQLite and immutable bytes.
use crate::repository_fs::{FsObjectStore, MAX_BYTES, MIN_FREE};
use clap::{Subcommand, ValueEnum};
use elzevir_app::{
    repository::{self as r, Clock},
    staff::{Permission, StaffId, StaffIdentity},
};
use elzevir_store::repository::MetadataStore;
use std::{
    error::Error,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(clap::Args)]
pub(crate) struct Args {
    /// Initialized institution directory. Stop its server before running a command.
    #[arg(long)]
    data_dir: PathBuf,
    #[arg(long, default_value_t=MAX_BYTES)]
    max_bytes: u64,
    #[arg(long, default_value_t=MIN_FREE)]
    min_free_bytes: u64,
    #[command(subcommand)]
    action: Action,
}
#[derive(Clone, Copy, ValueEnum)]
enum Role {
    Primary,
    Supplement,
    Administrative,
    Label,
}
impl From<Role> for r::FileRole {
    fn from(role: Role) -> Self {
        match role {
            Role::Primary => Self::Primary,
            Role::Supplement => Self::Supplement,
            Role::Administrative => Self::Administrative,
            Role::Label => Self::Label,
        }
    }
}
#[derive(Clone, Copy, ValueEnum)]
enum Access {
    Public,
    Staff,
}
#[derive(Clone, Copy)]
struct Id(u128);
impl std::str::FromStr for Id {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("expected a 32-digit hexadecimal ID".into());
        }
        u128::from_str_radix(s, 16)
            .map(Self)
            .map_err(|e| e.to_string())
    }
}
fn new_id() -> Result<u128, getrandom::Error> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes)?;
    Ok(u128::from_be_bytes(bytes))
}
#[derive(Subcommand)]
enum Action {
    /// Create a draft for an existing catalog instance. Prints its deposit ID.
    Draft { instance: i64 },
    /// Create a digital collection. Prints its ID.
    Collection { name: String },
    /// Associate a deposit with a collection.
    Join { deposit: Id, collection: Id },
    /// Add a logical file. Prints its ID.
    AddFile {
        deposit: Id,
        label: String,
        role: Role,
        access: Access,
    },
    /// Install a new immutable version. Use - for standard input.
    Upload(UploadArgs),
    /// Publish after verifying the current principal file.
    Publish { deposit: Id },
    /// Withdraw a published deposit, retaining its versions.
    Withdraw { deposit: Id },
    /// Withdraw an exact version.
    Retire {
        deposit: Id,
        file: Id,
        seq: u64,
        #[arg(long)]
        withdraw_deposit: bool,
    },
    /// Inspect a deposit. Use --public for the OPAC projection.
    Show {
        deposit: Id,
        #[arg(long)]
        public: bool,
    },
    /// Copy verified bytes to stdout. Staff reads are audited.
    Download {
        deposit: Id,
        file: Id,
        #[arg(long)]
        seq: Option<u64>,
        #[arg(long)]
        public: bool,
    },
    /// Verify an exact version and report affected references.
    Verify { deposit: Id, file: Id, seq: u64 },
    /// Count eligible files and bytes. --apply records observations and collects.
    Maintain {
        #[arg(long)]
        apply: bool,
    },
}
#[derive(clap::Args)]
struct UploadArgs {
    deposit: Id,
    file: Id,
    input: PathBuf,
    /// Supply the same 32-digit key and parameters when retrying.
    #[arg(long)]
    operation: Id,
    /// Omit only when the logical file has no current version.
    #[arg(long)]
    expected_current_seq: Option<u64>,
    #[arg(long)]
    name: String,
    #[arg(long)]
    media_type: String,
    #[arg(long)]
    sha256: Option<r::ObjectHash>,
}
struct WallClock;
impl Clock for WallClock {
    fn now(&self) -> i64 {
        jiff::Timestamp::now().as_second()
    }
}
fn audience(public: bool, actor: r::Operator) -> r::ReadAudience {
    if public {
        r::ReadAudience::Public
    } else {
        r::ReadAudience::Staff(actor)
    }
}
fn offline_actor() -> r::Result<r::Operator> {
    r::Operator::try_from(&StaffIdentity {
        id: StaffId(0),
        login: "offline".into(),
        permissions: [Permission::StaffInterface, Permission::Cataloguing].into(),
    })
}
type Pilot = r::Repository<MetadataStore, FsObjectStore, WallClock>;

pub(crate) fn run(args: Args) -> Result<(), String> {
    let lock = crate::backup_cli::lock(&args.data_dir)?;
    if crate::restore_journal::pending(&args.data_dir) {
        return Err("recover the interrupted restore before repository operations".into());
    }
    let objects = FsObjectStore::new(&args.data_dir, lock, args.max_bytes, args.min_free_bytes)
        .map_err(|e| e.to_string())?;
    let metadata =
        MetadataStore::open(&args.data_dir.join("elzevir.db")).map_err(|e| e.to_string())?;
    execute(
        &mut Pilot {
            metadata,
            objects,
            clock: WallClock,
        },
        args.action,
    )
    .map_err(|e| e.to_string())
}
fn execute(repo: &mut Pilot, action: Action) -> Result<(), Box<dyn Error>> {
    let actor = offline_actor()?;
    match action {
        Action::Draft { instance } => {
            let id = new_id().map_err(|e| e.to_string())?;
            repo.create_draft(actor, r::DepositId(id), r::InstanceId(instance))?;
            println!("{id:032x}");
        }
        Action::Collection { name } => {
            let id = new_id().map_err(|e| e.to_string())?;
            repo.metadata.create_collection(
                actor,
                repo.clock.now(),
                r::DigitalCollectionId(id),
                name,
            )?;
            println!("{id:032x}");
        }
        Action::Join {
            deposit,
            collection,
        } => repo.associate_collection(
            actor,
            r::DepositId(deposit.0),
            r::DigitalCollectionId(collection.0),
        )?,
        Action::AddFile {
            deposit,
            label,
            role,
            access,
        } => {
            let id = new_id().map_err(|e| e.to_string())?;
            repo.add_file(
                actor,
                r::DepositId(deposit.0),
                r::FileDescription {
                    id: r::FileId(id),
                    label,
                    role: role.into(),
                    access: match access {
                        Access::Staff => r::FileAccess::Staff,
                        Access::Public => r::FileAccess::Public,
                    },
                },
            )?;
            println!("{id:032x}");
        }
        Action::Upload(args) => upload(repo, actor, args)?,
        Action::Publish { deposit } => repo.publish(actor, r::DepositId(deposit.0))?,
        Action::Withdraw { deposit } => repo.withdraw(actor, r::DepositId(deposit.0))?,
        Action::Retire {
            deposit,
            file,
            seq,
            withdraw_deposit,
        } => repo.withdraw_version(
            actor,
            r::VersionRef {
                deposit: r::DepositId(deposit.0),
                file: r::FileId(file.0),
                seq,
            },
            withdraw_deposit,
        )?,
        Action::Show { deposit, public } => show(repo, actor, deposit, public)?,
        Action::Download {
            deposit,
            file,
            seq,
            public,
        } => {
            let mut download = repo.download(
                r::DepositId(deposit.0),
                r::FileId(file.0),
                seq,
                audience(public, actor),
            )?;
            io::copy(&mut download.reader, &mut io::stdout().lock())?;
        }
        Action::Verify { deposit, file, seq } => verify(repo, actor, deposit, file, seq)?,
        Action::Maintain { apply } => {
            let result = repo
                .objects
                .maintain(&repo.metadata, repo.clock.now(), apply)?;
            println!(
                "applied={apply} first_observations={} temporary={} orphan={} recoverable_bytes={}",
                result.observed, result.temporary, result.orphan, result.bytes
            );
        }
    }
    Ok(())
}

fn upload(repo: &Pilot, actor: r::Operator, args: UploadArgs) -> Result<(), Box<dyn Error>> {
    let UploadArgs {
        deposit,
        file,
        input,
        operation,
        expected_current_seq,
        name,
        media_type,
        sha256,
    } = args;
    let mut source: Box<dyn Read> = if input == Path::new("-") {
        Box::new(io::stdin().lock())
    } else {
        Box::new(File::open(input)?)
    };
    let receipt = repo.upload(
        actor,
        r::Upload {
            deposit: r::DepositId(deposit.0),
            file: r::FileId(file.0),
            operation: r::OperationId(operation.0),
            expected_current_seq,
            display_name: name,
            media_type,
        },
        &mut source,
        sha256,
    )?;
    println!(
        "seq={} sha256={} bytes={}",
        receipt.version.seq, receipt.object.hash, receipt.object.bytes
    );

    Ok(())
}
fn show(repo: &Pilot, actor: r::Operator, deposit: Id, public: bool) -> r::Result<()> {
    let page = repo.page(r::DepositId(deposit.0), audience(public, actor))?;
    println!(
        "deposit={:032x} instance={} state={:?}",
        page.id.0, page.instance.0, page.state
    );
    for id in page.collections {
        println!("collection={:032x}", id.0);
    }
    for (f, v) in page.files {
        println!(
            "file={:032x} role={:?} access={:?} current={:?} label={:?}",
            f.id.0,
            f.role,
            f.access,
            v.map(|v| v.seq),
            f.label
        );
    }

    Ok(())
}
fn verify(repo: &Pilot, actor: r::Operator, deposit: Id, file: Id, seq: u64) -> r::Result<()> {
    let version = r::VersionRef {
        deposit: r::DepositId(deposit.0),
        file: r::FileId(file.0),
        seq,
    };
    let (status, refs) = repo.diagnose(actor, version)?;
    repo.metadata
        .record_integrity(actor, repo.clock.now(), version, status)?;
    println!("integrity={status:?} references={}", refs.len());
    for v in refs {
        println!(
            "deposit={:032x} file={:032x} seq={}",
            v.deposit.0, v.file.0, v.seq
        );
    }
    if status != r::Integrity::Ok {
        return Err(r::RepositoryError::ObjectUnavailable);
    }

    Ok(())
}

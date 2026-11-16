// SPDX-License-Identifier: GPL-3.0-or-later
//! Listener and Host boundaries apply before any staff handler.

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    routing::get,
};
use elzevir_web::{AccessPolicy, restrict_access};
use tower::ServiceExt;

fn app(policy: AccessPolicy) -> Router {
    restrict_access(
        Router::new()
            .route("/", get(|| async { "catalog" }))
            .route(
                "/equipe/login",
                get(|| async { "staff" }).post(|| async { "staff" }),
            )
            .route("/static/style.css", get(|| async { "asset" })),
        policy,
    )
}

#[tokio::test]
async fn hostname_policy_is_enforced_for_staff_posts_assets_and_absolute_uris() {
    let router = app(AccessPolicy::hostnames("catalog.example", "staff.example").unwrap());
    for (path, host, expected) in [
        ("/", "catalog.example:8080", StatusCode::OK),
        ("/equipe/login", "catalog.example", StatusCode::NOT_FOUND),
        ("/equipe/login", "STAFF.EXAMPLE.", StatusCode::OK),
        ("/", "staff.example", StatusCode::NOT_FOUND),
        ("/static/style.css", "staff.example", StatusCode::OK),
        ("/equipe/login", "other.example", StatusCode::NOT_FOUND),
        (
            "http://catalog.example/equipe/login",
            "staff.example",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::post(path)
                    .method(if path == "/equipe/login" {
                        "POST"
                    } else {
                        "GET"
                    })
                    .header("host", host)
                    .header("x-forwarded-host", "staff.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{path} {host}");
        assert!(response.headers().contains_key("x-content-type-options"));
    }
    let duplicate = Request::get("/equipe/login")
        .header("host", "staff.example")
        .header("host", "catalog.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(duplicate).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn malformed_authorities_and_encoded_paths_cannot_reach_staff() {
    let router = app(AccessPolicy::hostnames("catalog.example", "staff.example").unwrap());
    for host in [
        "staff.example:0",
        "staff.example:abc",
        "staff.example:65536",
        "user@staff.example",
        "staff.example..",
    ] {
        let request = Request::get("/equipe/login")
            .header("host", host)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::BAD_REQUEST,
            "{host}"
        );
    }
    for path in [
        "/eq%75ipe/login",
        "/equipe%2flogin",
        "//equipe/login",
        "/equipe/login",
    ] {
        let request = Request::get(path)
            .header("host", "catalog.example")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

#[tokio::test]
async fn separate_listener_uses_the_tcp_peer_and_ignores_forwarded_addresses() {
    let catalog = app(AccessPolicy::catalog_only());
    assert_eq!(
        catalog
            .oneshot(Request::get("/equipe/login").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let staff =
        app(
            AccessPolicy::staff_only(&["192.0.2.0/24".to_owned(), "2001:db8::/32".to_owned()])
                .unwrap(),
        );
    for (peer, expected) in [
        (Some("192.0.2.10:9000"), StatusCode::OK),
        (Some("[::ffff:192.0.2.10]:9000"), StatusCode::OK),
        (Some("[2001:db8::10]:9000"), StatusCode::OK),
        (Some("198.51.100.10:9000"), StatusCode::FORBIDDEN),
        (None, StatusCode::FORBIDDEN),
    ] {
        let mut request = Request::get("/equipe/login")
            .header("x-forwarded-for", "192.0.2.10")
            .body(Body::empty())
            .unwrap();
        if let Some(peer) = peer {
            request
                .extensions_mut()
                .insert(ConnectInfo(peer.parse::<std::net::SocketAddr>().unwrap()));
        }
        assert_eq!(
            staff.clone().oneshot(request).await.unwrap().status(),
            expected
        );
    }
    let unrestricted = app(AccessPolicy::staff_only(&[]).unwrap());
    assert_eq!(
        unrestricted
            .clone()
            .oneshot(Request::get("/equipe/login").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        unrestricted
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn invalid_hosts_networks_and_oversized_lists_are_refused() {
    for host in [
        "https://staff.example",
        "staff.example/path",
        "staff.example:80",
        "user@staff.example",
        "*.example",
        "a..example",
        "-bad.example",
        "staff\r\n.example",
        "équipe.example",
    ] {
        assert!(AccessPolicy::hostnames("catalog.example", host).is_err());
    }
    assert!(AccessPolicy::hostnames("EXAMPLE.", "example").is_err());
    for network in [
        "192.0.2.0/33",
        "::/129",
        "any",
        "192.0.2.1/-1",
        "192.0.2.0/24/trailing",
    ] {
        assert!(AccessPolicy::staff_only(&[network.to_owned()]).is_err());
    }
    assert!(AccessPolicy::staff_only(&vec!["127.0.0.1".to_owned(); 33]).is_err());
}

#[tokio::test]
async fn split_navigation_redirects_without_serving_staff_handlers_on_catalog() {
    let hosts = app(AccessPolicy::hostnames("catalog.example", "staff.example")
        .unwrap()
        .with_listener_addresses(
            "0.0.0.0:8080".parse().unwrap(),
            "0.0.0.0:8080".parse().unwrap(),
        ));
    let ports = app(AccessPolicy::catalog_only().with_listener_addresses(
        "[::]:8080".parse().unwrap(),
        "0.0.0.0:8081".parse().unwrap(),
    ));
    let staff = app(AccessPolicy::staff_only(&[])
        .unwrap()
        .with_listener_addresses(
            "[::]:8080".parse().unwrap(),
            "0.0.0.0:8081".parse().unwrap(),
        ));
    for (router, host, path, target) in [
        (
            hosts.clone(),
            "catalog.example",
            "/equipe",
            "//staff.example:8080/equipe",
        ),
        (
            hosts,
            "staff.example",
            "/record/1?format=turtle",
            "//catalog.example:8080/record/1?format=turtle",
        ),
        (
            ports,
            "localhost:8080",
            "/equipe",
            "//localhost:8081/equipe",
        ),
        (staff, "[::1]:8081", "/", "//[::1]:8080/"),
    ] {
        let response = router
            .oneshot(
                Request::get(path)
                    .header("host", host)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.headers().get("location").unwrap(), target);
    }
}

#[tokio::test]
async fn navigation_keeps_methods_hosts_and_staff_routes_restricted() {
    let catalog = "0.0.0.0:8080".parse().unwrap();
    let staff = "0.0.0.0:8081".parse().unwrap();
    let hosts = AccessPolicy::hostnames("catalog.example", "staff.example")
        .unwrap()
        .with_listener_addresses(catalog, staff);
    let ports = AccessPolicy::catalog_only().with_listener_addresses(catalog, staff);
    for (policy, method, path, host, expected) in [
        (
            hosts.clone(),
            "POST",
            "/equipe",
            "catalog.example",
            StatusCode::NOT_FOUND,
        ),
        (
            hosts.clone(),
            "GET",
            "/equipe/login",
            "catalog.example",
            StatusCode::NOT_FOUND,
        ),
        (
            hosts.clone(),
            "GET",
            "/equipe",
            "unknown.example",
            StatusCode::NOT_FOUND,
        ),
        (
            hosts,
            "GET",
            "/equipe",
            "catalog.example:abc",
            StatusCode::BAD_REQUEST,
        ),
        (
            ports.clone(),
            "POST",
            "/equipe",
            "localhost",
            StatusCode::NOT_FOUND,
        ),
        (
            ports.clone(),
            "GET",
            "/equipe/login",
            "localhost",
            StatusCode::NOT_FOUND,
        ),
        (
            ports.clone(),
            "GET",
            "/equipe",
            "user@localhost",
            StatusCode::BAD_REQUEST,
        ),
        (
            ports.clone(),
            "GET",
            "/equipe",
            "localhost:0",
            StatusCode::BAD_REQUEST,
        ),
        (
            ports
                .clone()
                .with_listener_addresses(catalog, "127.0.0.1:0".parse().unwrap()),
            "GET",
            "/equipe",
            "localhost",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let response = app(policy)
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", host)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{method} {path} {host}");
        assert!(!response.headers().contains_key("location"));
        assert!(response.headers().contains_key("x-content-type-options"));
    }
    let duplicate = Request::get("/equipe")
        .header("host", "localhost")
        .header("host", "other.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app(ports).oneshot(duplicate).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn specific_listener_address_wins_over_request_host() {
    for (destination, expected) in [
        ("127.0.0.1:8081", "//127.0.0.1:8081/equipe"),
        ("[::1]:8081", "//[::1]:8081/equipe"),
    ] {
        let policy = AccessPolicy::catalog_only().with_listener_addresses(
            "127.0.0.1:8080".parse().unwrap(),
            destination.parse().unwrap(),
        );
        let response = app(policy)
            .oneshot(
                Request::get("/equipe")
                    .header("host", "other.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.headers().get("location").unwrap(), expected);
    }
}

//! Combined init container for the platform service mesh.
//!
//! 1. Copies the baked-in `/platform-proxy` binary to the shared emptyDir volume
//!    at `/proxy/platform-proxy` so application containers can use it.
//! 2. Sets up iptables REDIRECT rules for transparent traffic interception.
//!
//! Runs in a distroless image (no `/bin/sh`) to minimize attack surface despite
//! requiring `NET_ADMIN` capability for iptables.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn main() {
    // --- Step 1: Copy proxy binary to shared volume ---
    println!("[proxy-init] Copying platform-proxy to /proxy/...");
    fs::copy("/platform-proxy", "/proxy/platform-proxy")
        .expect("[proxy-init] failed to copy /platform-proxy to /proxy/platform-proxy");
    fs::set_permissions("/proxy/platform-proxy", fs::Permissions::from_mode(0o755))
        .expect("[proxy-init] failed to chmod /proxy/platform-proxy");

    // --- Step 2: Set up iptables rules ---
    println!("[proxy-init] Setting up iptables rules...");

    let inbound_port = env_or("PROXY_INBOUND_PORT", "15006");
    let outbound_port = env_or("PROXY_OUTBOUND_PORT", "15001");
    let health_port = env_or("PROXY_HEALTH_PORT", "15020");
    let outbound_bind = env_or("PROXY_OUTBOUND_BIND", "127.0.0.6");
    let outbound_cidr = format!("{outbound_bind}/32");

    // INBOUND: redirect external TCP to proxy inbound listener
    ipt(&["-t", "nat", "-N", "PLATFORM_INBOUND"]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PLATFORM_INBOUND",
        "-p",
        "tcp",
        "--dport",
        &inbound_port,
        "-j",
        "RETURN",
    ]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PLATFORM_INBOUND",
        "-p",
        "tcp",
        "--dport",
        &outbound_port,
        "-j",
        "RETURN",
    ]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PLATFORM_INBOUND",
        "-p",
        "tcp",
        "--dport",
        &health_port,
        "-j",
        "RETURN",
    ]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PLATFORM_INBOUND",
        "-p",
        "tcp",
        "-j",
        "REDIRECT",
        "--to-ports",
        &inbound_port,
    ]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PREROUTING",
        "-p",
        "tcp",
        "-j",
        "PLATFORM_INBOUND",
    ]);

    // OUTBOUND: redirect app-originated TCP to proxy outbound listener
    ipt(&["-t", "nat", "-N", "PLATFORM_OUTPUT"]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PLATFORM_OUTPUT",
        "-s",
        &outbound_cidr,
        "-j",
        "RETURN",
    ]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PLATFORM_OUTPUT",
        "-o",
        "lo",
        "-d",
        "127.0.0.1/32",
        "-j",
        "RETURN",
    ]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PLATFORM_OUTPUT",
        "-p",
        "tcp",
        "--dport",
        "53",
        "-j",
        "RETURN",
    ]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "PLATFORM_OUTPUT",
        "-p",
        "tcp",
        "-j",
        "REDIRECT",
        "--to-ports",
        &outbound_port,
    ]);
    ipt(&[
        "-t",
        "nat",
        "-A",
        "OUTPUT",
        "-p",
        "tcp",
        "-j",
        "PLATFORM_OUTPUT",
    ]);

    println!("[proxy-init] Ready (inbound:{inbound_port} outbound:{outbound_port})");
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn ipt(args: &[&str]) {
    let status = Command::new("iptables")
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("[proxy-init] failed to execute iptables: {e}"));

    assert!(
        status.success(),
        "[proxy-init] iptables {args:?} exited with {status}"
    );
}

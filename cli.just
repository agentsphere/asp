# cli.just — Agent Runner CLI recipes

worktree := `bash hack/detect-worktree.sh`
agent_runner_dir := "/tmp/platform-e2e/" + worktree + "/agent-runner"

[group('cli')]
build:
    cargo build --release -p platform-agent-runner

# Cross-compile agent-runner for linux/amd64 and linux/arm64 (uses Docker)
[group('cli')]
cross dir=agent_runner_dir:
    mkdir -p {{ dir }}
    docker run --rm \
      -v "$(pwd):/src" \
      -v "{{ dir }}:/out" \
      -v "platform-cross-cargo-registry:/usr/local/cargo/registry" \
      -v "platform-cross-cargo-git:/usr/local/cargo/git" \
      -v "platform-cross-target:/src/target" \
      rust:1.88-slim-bookworm sh -c '\
        apt-get update && apt-get install -y --no-install-recommends \
          gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
          gcc-x86-64-linux-gnu libc6-dev-amd64-cross && \
        rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu && \
        cd /src && \
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
          cargo build --release -p platform-agent-runner --target aarch64-unknown-linux-gnu && \
        CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
          cargo build --release -p platform-agent-runner --target x86_64-unknown-linux-gnu && \
        cp target/aarch64-unknown-linux-gnu/release/agent-runner /out/arm64 && \
        cp target/x86_64-unknown-linux-gnu/release/agent-runner /out/amd64'

[group('cli')]
install:
    cargo install --path crates/bins/platform-agent-runner

# `just cli test` or `just cli test some_test_name`
[group('cli')]
test *args="":
    cargo test -p platform-agent-runner --bin agent-runner {{args}}

[group('cli')]
lint:
    cargo clippy -p platform-agent-runner --all-features -- -D warnings

[group('cli')]
fmt:
    cargo fmt -p platform-agent-runner

# CLI pub/sub integration test (requires cluster)
[group('cli')]
test-pubsub:
    bash hack/cli-test-pubsub.sh

# Real Claude CLI tests (requires CLAUDE_OAUTH_TOKEN)
[group('cli')]
test-llm:
    cargo test -p platform-agent-runner --bin agent-runner llm_ -- --ignored

[group('cli')]
cov:
    cargo llvm-cov -p platform-agent-runner --bin agent-runner

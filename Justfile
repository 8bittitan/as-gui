set dotenv-load
set shell := ["bash", "-euo", "pipefail", "-c"]

set default-list := true

build:
  cargo build --release --locked --no-default-features

fmt:
  cargo fmt

lint:
  cargo clippy --all-targets --locked --no-default-features -- -D warnings

test:
  cargo test --locked --no-default-features

check:
  cargo fmt --check
  just lint
  just test

dev:
  cargo run

# Records a demo video into demos/: the main tour by default, or quick-ask, menu-bar, ask-selection, notify or all.
demo name="tour":
  scripts/demo.sh "{{name}}"

bundle:
  packaging/bundle.sh

dmg:
  packaging/bundle.sh dmg

screenshots:
  AS_GUI_DEMO=1 cargo run --release --locked --no-default-features -- --snap docs/conversation.png --scene conversation --size 1100x720 --theme light
  AS_GUI_DEMO=1 cargo run --release --locked --no-default-features -- --snap docs/search-panel.png --scene search --size 1100x720 --theme light
  AS_GUI_DEMO=1 cargo run --release --locked --no-default-features -- --snap docs/command-bar.png --scene command-bar --size 1100x720 --theme light

changelog:
  git cliff -u -o CHANGELOG.md

release version:
  scripts/release.sh "{{version}}"

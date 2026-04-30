# TUI Example

This one at the moment takes midi in, plays an FM synth with a bit of echo, and renders a basic oscilloscope. I hope to build this up over time into a multi-component FM sequencer.

You need a similar toolchain to the upstream Legato repo, 
e.g Rust nightly.

```sh
# If you use direnv and flakes, just run this alias
run-release
# Otherwise, install a Rust nightly toolchain, and run the following...
cargo run --release --manifest-path ./src-legato/Cargo.toml
```

You may have to play around in main.rs to change your audio settings, i.e block size, sample rate, etc. In the future, there will be an environmental or builder configuration for this.
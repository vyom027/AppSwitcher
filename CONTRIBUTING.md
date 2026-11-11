# Contributing to AppSwitcher

Thanks for trying AppSwitcher. Feedback, bug reports, design ideas, and code contributions are welcome.

## Feedback

Use [Discussions](https://github.com/vyom027/AppSwitcher/discussions) for impressions, layout and animation ideas, and questions. Use [Issues](https://github.com/vyom027/AppSwitcher/issues/new/choose) for bugs with steps to reproduce.

When reporting a gesture or rendering issue, include your Windows version, touchpad model, screen resolution and scaling, selected layout and animation, and what you expected to happen. Please remove personal information from screenshots and logs.

## Code changes

1. Fork this repository and create a branch for your change.
2. Build the Rust app from the `rust` directory with `cargo build --release` on 64-bit Windows using the MSVC Rust toolchain and Windows SDK.
3. Explain the user-visible effect in your pull request. Include a screenshot or short recording for UI changes.
4. Keep changes focused so they are easy to review.

The Python implementation is retained for reference. New feature work belongs in `rust/`.

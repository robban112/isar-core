#!/bin/bash

rustup target add aarch64-apple-ios x86_64-apple-ios

cargo install cargo-lipo

cargo lipo --targets aarch64-apple-ios x86_64-apple-ios --release

mv "target/universal/release/libisar.a" "target/universal/release/libisar_ios.a"
#!/usr/bin/env bash
# cargo の runner: iOS シミュレータ向けにビルドしたテストの実行ファイルを、起動中のシミュレータの中で実行する。
# 使い方: CARGO_TARGET_AARCH64_APPLE_IOS_SIM_RUNNER=scripts/ci/ios-sim-runner.sh cargo test --target aarch64-apple-ios-sim ...
# （シミュレータは先に起動しておく。.github/workflows/mobile.yml の ios ジョブ）
exec xcrun simctl spawn booted "$@"

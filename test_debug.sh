#!/bin/bash
RUST_LOG=debug ./atomc test_mutable.atom --no-std 2>&1 | grep -A5 -B5 "found"

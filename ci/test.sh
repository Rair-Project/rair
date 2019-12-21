#!/bin/sh
set -e

if [ $COV ]; then
  set +e # cargo install might fail because package already exist in cache
  cargo install cargo-tarpaulin
  set -e
  cargo test --all --doc # testing doc tests
  cargo tarpaulin --ignore-tests -l --all --out Xml # testing and code coveage
  curl -s https://codecov.io/bash | bash
else
  cargo test --all
fi

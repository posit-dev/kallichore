#!/usr/bin/env bash

# Test to see if openapi-generator is installed
if ! command -v openapi-generator &> /dev/null
then
    echo "openapi-generator could not be found"
    echo "Please install openapi-generator-cli"
    echo "https://openapi-generator.tech/docs/installation"
    exit
fi

# Get the path to this script
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

# Move to the directory containing the API implementation
cd "$DIR/../crates/kallichore_api"

# Generate the API
openapi-generator generate -i ../../kallichore.json -g rust-server --additional-properties=packageName=kallichore_api

# Remove unwanted TLS/HTTPS/OpenSSL dependencies
"$DIR/remove-tls-deps.sh"

# Format all of the generated Rust code
cargo fmt

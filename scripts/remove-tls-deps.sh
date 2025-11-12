#!/usr/bin/env bash

# Post-processing script to remove unwanted TLS/HTTPS/OpenSSL dependencies
# that the OpenAPI generator automatically adds to the generated code.
#
# This script is run automatically by regen-api.sh after API generation.

set -e

# Get the path to this script
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

# Path to the kallichore_api Cargo.toml
CARGO_TOML="$DIR/../crates/kallichore_api/Cargo.toml"

echo "Removing unwanted TLS/HTTPS/OpenSSL dependencies from kallichore_api..."

# Create a temporary file
TMP_FILE=$(mktemp)

# Use awk to process the Cargo.toml file
awk '
BEGIN {
    in_target_section = 0
    skip_until_next_section = 0
}

# Handle the client feature line
/^client = \[/ {
    # Start multi-line handling
    buffer = $0
    if (index($0, "]") == 0) {
        # Multi-line feature list
        getline
        while (index($0, "]") == 0) {
            buffer = buffer "\n" $0
            getline
        }
        buffer = buffer "\n" $0
    }
    # Replace the entire client feature line
    print "client = ["
    print "    \"hyper\", \"hyper-util/http1\", \"hyper-util/http2\", \"url\""
    print "]"
    next
}

# Handle swagger dependency line - replace "tls" with nothing (remove it from features)
/^swagger = \{.*features = \[.*"tls"/ {
    gsub(/"tls"/, "")
    # Clean up any resulting ", ]" or "[, " patterns
    gsub(/\[, /, "[")
    gsub(/, \]/, "]")
    gsub(/,  /, ", ")
    print
    next
}

# Detect platform-specific TLS dependency sections and skip them
/^\[target\.\x27cfg\(any\(target_os = "macos"/ {
    in_target_section = 1
    skip_until_next_section = 1
    next
}

/^\[target\.\x27cfg\(not\(any\(target_os/ {
    in_target_section = 1
    skip_until_next_section = 1
    next
}

# Skip lines in target sections that contain TLS dependencies
skip_until_next_section == 1 {
    # Check if we hit a new section
    if (/^\[/ && !/^\[target\./) {
        # New non-target section, stop skipping
        skip_until_next_section = 0
        in_target_section = 0
        print
    } else if (/^$/) {
        # Empty line within target section, skip it
        next
    } else if (/native-tls/ || /hyper-tls/ || /hyper-openssl/ || /openssl/) {
        # Skip TLS dependency lines
        next
    }
    next
}

# Remove native-tls and openssl from dev-dependencies
/^\[.*dev-dependencies\]/ {
    print
    in_dev_deps = 1
    next
}

in_dev_deps == 1 && /^\[/ {
    in_dev_deps = 0
}

in_dev_deps == 1 && (/^native-tls/ || /^openssl/ || /^tokio-openssl/) {
    next
}

# Print all other lines
{
    print
}
' "$CARGO_TOML" > "$TMP_FILE"

# Replace the original file
mv "$TMP_FILE" "$CARGO_TOML"

# Also fix kcshared to not pull in default features
KCSHARED_CARGO="$DIR/../crates/kcshared/Cargo.toml"

if grep -q '^kallichore_api = { path = "../kallichore_api" }$' "$KCSHARED_CARGO" 2>/dev/null; then
    echo "Fixing kcshared to not pull in kallichore_api client feature..."
    sed -i.bak 's|^kallichore_api = { path = "../kallichore_api" }$|kallichore_api = { path = "../kallichore_api", default-features = false, features = ["server"] }|' "$KCSHARED_CARGO"
    rm -f "$KCSHARED_CARGO.bak"
fi

echo "TLS dependencies removed successfully!"
echo ""
echo "Summary of changes:"
echo "  - Removed hyper-openssl, hyper-tls, native-tls, openssl from client feature"
echo "  - Removed 'tls' from swagger features"
echo "  - Removed platform-specific TLS dependency sections"
echo "  - Removed TLS dependencies from dev-dependencies"
echo "  - Fixed kcshared to use server-only features"

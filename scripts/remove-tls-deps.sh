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

# Remove HTTPS/TLS-specific code from client/mod.rs while keeping HTTP functionality
CLIENT_MOD="$DIR/../crates/kallichore_api/src/client/mod.rs"
if [ -f "$CLIENT_MOD" ]; then
    echo "Removing HTTPS/TLS code from client/mod.rs..."

    python3 - "$CLIENT_MOD" "$DIR/../crates/kallichore_api/examples/client/main.rs" "$DIR/../crates/kallichore_api/examples/server/main.rs" <<'PYTHON_SCRIPT'
import sys
import re

def remove_tls_from_client(filepath):
    with open(filepath, 'r') as f:
        lines = f.readlines()

    output = []
    i = 0
    skip_until_brace = False
    brace_count = 0
    in_https_match_arm = False
    https_arm_brace_count = 0

    while i < len(lines):
        line = lines[i]

        # Skip HttpsConnector type definitions (with cfg attributes)
        if re.match(r'^#\[cfg\(.*target_os.*\)\]', line):
            if i + 1 < len(lines) and 'HttpsConnector' in lines[i + 1]:
                # Skip cfg and type definition
                i += 1
                while i < len(lines) and not lines[i].rstrip().endswith(';'):
                    i += 1
                i += 1  # Skip the semicolon line
                continue

        # Skip standalone HttpsConnector type definitions
        if line.startswith('type HttpsConnector'):
            while i < len(lines) and not lines[i].rstrip().endswith(';'):
                i += 1
            i += 1
            continue

        # Remove Https variant from HyperClient enum
        if 'Https(hyper_util::client::legacy::Client<HttpsConnector' in line:
            i += 1
            continue

        # Remove match arms that reference HyperClient::Https
        if 'HyperClient::Https' in line:
            i += 1
            continue

        # Skip impl blocks for HttpsConnector
        if line.startswith('impl<C>'):
            # Look ahead to see if this impl is for HttpsConnector
            lookahead = ''.join(lines[i:min(i+15, len(lines))])
            if 'HttpsConnector' in lookahead:
                # Skip entire impl block
                brace_count = 0
                i += 1
                while i < len(lines):
                    if '{' in lines[i]:
                        brace_count += lines[i].count('{')
                    if '}' in lines[i]:
                        brace_count -= lines[i].count('}')
                    i += 1
                    if brace_count == 0 and lines[i-1].strip() == '}':
                        break
                continue

        # Handle https match arm in try_new_with_connector
        if re.match(r'^\s+"https"\s*=>\s*\{', line):
            output.append('            // "https" => { // HTTPS support removed\n')
            in_https_match_arm = True
            https_arm_brace_count = 1
            i += 1
            continue

        if in_https_match_arm:
            https_arm_brace_count += line.count('{')
            https_arm_brace_count -= line.count('}')
            output.append('            // ' + line)
            if https_arm_brace_count == 0:
                in_https_match_arm = False
            i += 1
            continue

        # Handle ClientInitError enum - remove SSL variants
        if 'pub enum ClientInitError' in line:
            output.append(line)
            i += 1
            # Process enum contents
            while i < len(lines) and not (lines[i].strip() == '}' and 'enum' not in lines[i]):
                if '/// SSL Connection Error' in lines[i]:
                    # Skip doc comments and SslError variant
                    while i < len(lines):
                        if 'SslError' in lines[i]:
                            i += 1
                            break
                        elif lines[i].strip().startswith('///') or lines[i].strip().startswith('#[cfg') or lines[i].strip() == '':
                            i += 1
                        else:
                            break
                    continue
                output.append(lines[i])
                i += 1
            if i < len(lines):
                output.append(lines[i])  # closing brace
            i += 1
            continue

        # Remove ClientInitError::SslError references
        if 'ClientInitError::SslError' in line:
            # Comment out the line
            output.append('//' + line)
            i += 1
            continue

        # Remove .https() calls
        if '.https()' in line and 'Connector::builder()' in ''.join(lines[max(0,i-2):i+1]):
            # Comment out lines with .https()
            output.append('//' + line)
            i += 1
            continue

        # Default: keep the line
        output.append(line)
        i += 1

    with open(filepath, 'w') as f:
        f.writelines(output)

def fix_example_files(filepath):
    """Fix example files to use HTTP instead of HTTPS"""
    with open(filepath, 'r') as f:
        content = f.read()

    # Replace try_new_https with try_new_http
    content = re.sub(r'try_new_https', 'try_new_http', content)
    content = re.sub(r'Failed to create HTTPS client', 'Failed to create HTTP client', content)

    # Replace https:// URLs with http://
    content = re.sub(r'https://localhost', 'http://localhost', content)
    content = re.sub(r'"https"', '"http"', content)

    # Comment out or replace HTTPS-specific code
    lines = content.split('\n')
    output = []
    skip_https_block = False

    for line in lines:
        # Skip blocks that are HTTPS-only
        if 'using_https' in line.lower() or 'if https' in line.lower():
            skip_https_block = True

        if skip_https_block:
            output.append('// ' + line)
            if '{' not in line and '}' in line:
                skip_https_block = False
        else:
            output.append(line)

    with open(filepath, 'w') as f:
        f.write('\n'.join(output))

if __name__ == '__main__':
    # Fix the main client module
    remove_tls_from_client(sys.argv[1])

    # Fix example files if they exist
    for example_file in sys.argv[2:]:
        try:
            fix_example_files(example_file)
        except FileNotFoundError:
            pass
PYTHON_SCRIPT

fi

echo "TLS dependencies removed successfully!"
echo ""
echo "Summary of changes:"
echo "  - Removed hyper-openssl, hyper-tls, native-tls, openssl from client feature"
echo "  - Removed 'tls' from swagger features"
echo "  - Removed platform-specific TLS dependency sections"
echo "  - Removed TLS dependencies from dev-dependencies"
echo "  - Fixed kcshared to use server-only features"
echo "  - Removed HTTPS/TLS code from client (kept HTTP support)"

//
// private_file.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Like `File::create`, but a newly created file is readable and writable only
/// by the current user.
pub fn create<P: AsRef<Path>>(path: P) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

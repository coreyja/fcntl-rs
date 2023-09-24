# Fork of https://gitlab.com/BafDyce/fcntl-rs

# fcntl-rs

> Wrapper around [fcntl (2)](https://www.man7.org/linux/man-pages/man2/fcntl.2.html) and convenience methods to make
> interacting with it easier. Currently only supports commands related to Advisory record locking.

- [Documentation](https://docs.rs/fcntl)
- [Crates.io](https://crates.io/crates/fcntl)
- [Source Code](https://gitlab.com/BafDyce/fcntl-rs)

## Usage

`Cargo.toml`

```toml
[dependencies]
fcntl = "0.1"
```

```rust
use std::fs::OpenOptions;
use fcntl::{is_file_locked, lock_file, unlock_file};

// Open file
let file = OpenOptions::new().read(true).open("my.lock").unwrap();

// Check whether any process is currently holding a lock
match is_file_locked(&file, None) {
    Ok(true) => println!("File is currently locked"),
    Ok(false) => println!("File is not locked"),
    Err(err) => println!("Error: {:?}", err),
}


// Attempt to acquire a lock
match lock_file(&file, None, Some(FcntlLockType::Write)) {
    Ok(true) => println!("Lock acquired!"),
    Ok(false) => println!("Could not acquire lock!"),
    Err(err) => println!("Error: {:?}", err),
}


// Release lock again
match unlock_file(&file, None) {
    Ok(true) => println!("Lock successfully release"),
    Ok(false) => println!("Failed to release lock"),
    Err(err) => println!("Error: {:?}", err),
}
```

## License

[MIT](./LICENSE-MIT) OR [Apache-2.0](./LICENSE-APACHE)

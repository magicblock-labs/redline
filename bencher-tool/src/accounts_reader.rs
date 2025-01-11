use std::{
    fs::File,
    io::{BufRead, BufReader, Seek},
    path::PathBuf,
    str::FromStr,
};

use solana::pubkey::Pubkey;

pub struct AccountReader {
    inner: BufReader<File>,
    buf: String,
}

impl AccountReader {
    pub fn new(path: PathBuf) -> Self {
        let inner = File::open(path).expect("failed to open accounts file");
        Self {
            inner: BufReader::with_capacity(512, inner),
            buf: String::with_capacity(128),
        }
    }
    pub fn next(&mut self, mut count: u8) -> Vec<Pubkey> {
        let mut pubkeys = Vec::with_capacity(count as usize);
        while count > 0 {
            self.buf.clear();
            self.inner
                .read_line(&mut self.buf)
                .expect("accounts file read error");
            // empty read of just a new line
            if (0..=2).contains(&self.buf.len()) {
                self.inner.rewind().expect("failed to rewind");
                eprintln!("run out of accounts, rewinding");
                continue;
            }
            let pk = Pubkey::from_str(&self.buf).expect("invalid pubkey in file");
            count -= 1;
            pubkeys.push(pk);
        }
        pubkeys
    }
}

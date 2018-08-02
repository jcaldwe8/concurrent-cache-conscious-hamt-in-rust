#![feature(test)]

#[macro_use]
extern crate cchamt;

extern crate test;
extern crate rand;

use std::usize;
use cchamt::{ContiguousTrie};

#[test]
fn test_new_trie() {
    let _trie = ContiguousTrie::<(usize)>::new(32,8);
}

#[test]
fn test_insert() {
    let mut trie = ContiguousTrie::<usize>::new(32, 8);

    let arr = binary_format!(4).to_owned().into_bytes();
    trie.insert(4, &arr[2..]);

    assert_eq!(trie.contain(&arr[2..]), true);
    assert_eq!(trie.get(&arr[2..]).unwrap(), 4);
}

#[test]
fn test_1e5_insert() {
    let mut trie = ContiguousTrie::<usize>::new(32, 8);

    for i in 0..100000 {
        let str = binary_format!(i);
//        println!("{}", str);
        let arr = str.to_owned().into_bytes();
        trie.insert(i, &arr[2..]);
    }

    for i in 0..100000 {
        let str = binary_format!(i);
        let arr = str.to_owned().into_bytes();
        assert_eq!(trie.get(&arr[2..]).unwrap(), i);
    }
}

#[test]
fn test_2pow24_insert() {
    let mut trie = ContiguousTrie::<usize>::new(32, 8);

    let tot: usize = 2usize.pow(24);

    for i in 0..tot {
        let str = binary_format!(i);
//        println!("{}", str);
        let arr = str.to_owned().into_bytes();
        trie.insert(i, &arr[2..]);
    }

    for i in 0..tot {
        let str = binary_format!(i);
        let arr = str.to_owned().into_bytes();
        assert_eq!(trie.get(&arr[2..]).unwrap(), i);
    }
}

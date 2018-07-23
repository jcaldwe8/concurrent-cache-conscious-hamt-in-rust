#![feature(test)]

#[macro_use]
extern crate cchamt;

extern crate test;
extern crate rand;

//use test::Bencher;
use std::usize;
//use std::collections::HashMap;
//use rand::{Rng, thread_rng};
//use rand::Rng;
use cchamt::MutexContiguousTrie;


#[test]
fn test_new_contiguous_trie() {
    let _trie = MutexContiguousTrie::<usize>::new(32, 8);
}

#[test]
fn test_2_power_16_insert() {
    let trie = MutexContiguousTrie::<usize>::new(32, 8);

    for i in 0..65536 {
        let str = binary_format!(i);
        let arr = str.to_owned().into_bytes();
        trie.insert(i, &arr[2..]);
    }

    for i in 0..65536 {
        let str = binary_format!(i);
        let arr = str.to_owned().into_bytes();
        assert_eq!(trie.get(&arr[2..]).unwrap(), i);
    }
}

#[test]
fn test_million_consecutive_insert() {
    let trie = MutexContiguousTrie::<usize>::new(32, 8);

    for i in 0..1000000 {
        let str = binary_format!(i);
        let arr = str.to_owned().into_bytes();
        trie.insert(i, &arr[2..]);
    }

    for i in 0..1000000 {
        let str = binary_format!(i);
        let arr = str.to_owned().into_bytes();
        assert_eq!(trie.get(&arr[2..]).unwrap(), i);
    }
}

/// Cache Conscious Hash Trie - Statically packing the entry contiguously
/// This file is simply for knowing the potential how cache conscious
/// could benefit hash trie.
///
/// The insert function is this file should be rewrite, or you can see the implementation in
/// `src/lockfree_cchamt.rs`
///
/// The benchmark is in:
/// https://github.com/chichunchen/concurrent-cache-conscious-hamt-in-rust/blob/layout/Benchmark.ipynb

use std::sync::{Arc, Mutex};
use std::thread;

pub trait TrieData: Clone + Copy + Eq + PartialEq {}

impl<T> TrieData for T where T: Clone + Copy + Eq + PartialEq {}

/// Private Functions for this module

/// compute the depth in the trie using the array index of trie.memory
// TODO bug here
#[inline(always)]
fn get_depth(key_group: usize, index: usize) -> usize {
    let mut depth = 0;
    let key_length = u32::pow(2, key_group as u32); // the number of distinct keys produced by the key group
    let mut multitude = key_length; // multitudes of key_length
    let mut compare = multitude; // sum of multitudes

    while index >= compare as usize {
        depth += 1;
        multitude *= key_length;
        compare += multitude;
    }//while
    depth
}//get_depth

/// Core Data structure
#[derive(Debug)]
pub struct ContiguousTrie<T: TrieData> {
    memory: Vec<Option<SubTrie<T>>>,
    key_length: usize,
    key_segment_size: usize,
}//struct ContiguousTrie


#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SubTrie<T: TrieData> {
    pub data: Option<T>,
    depth: usize,
    children_offset: Option<usize>,    // the start position in allocator that place the array in hash trie
}//struct SubTrie

// Contiguous store all the nodes contiguous with the sequential order of key
impl<T: TrieData> ContiguousTrie<T> {
    //constructor
    // key_length: length of the key
    // key_segment_size: length of a key segment (a key_group)
    pub fn new(key_length: usize, key_segment_size: usize) -> Self {
        // key_length needs to be multiple of key_segment_size
        assert_eq!(key_length % key_segment_size, 0);

        let mut memory: Vec<Option<SubTrie<T>>>; //memory is a vector that contains SubTries or None
        // init with all nodes that is not leaf
        // nodes_length = summation of KEY_LEN^1 to KEY_LEN^(KEY_LEN/KEY_GROUP-1)
        {//new block
            let mut nodes_length = 0;
            let array_length = usize::pow(2, key_segment_size as u32);
            let mut multitude = array_length;
            for _ in 0..(key_length / key_segment_size - 1) { //for the number of segments
                nodes_length += multitude;
                multitude *= array_length;
            }//for
//            println!("nl {}", nodes_length);
            memory = Vec::with_capacity(nodes_length); //total memory needed for this KEY_LEN and KEY_GROUP

            //for each index from 0 to the # of nodes
            for i in 0..nodes_length {
                memory.push(Some(SubTrie { //add a SubTrie in each index of the memory vector
                    data: None,
                    depth: get_depth(key_segment_size as usize, i),
                    children_offset: Some((i + 1) * array_length as usize),
                }));
//                println!("co {} {}", i, (i + 1) * array_length as usize);
            }//for
        }//end block

        //return this struct
        ContiguousTrie {
            memory,
            key_length,
            key_segment_size,
        }//return struct
    }//constructor

    // return the index in the first <= 4 bits
    // for instances: 0000 0000 -> 0
    #[inline(always)]
    fn compute_index(&self, key: &[u8]) -> usize {
        let mut id = 0;
        let length = if key.len() > self.key_segment_size { self.key_segment_size } else { key.len() };
        for i in 0..length {
            let temp = key[i] as usize - '0' as usize;
            id += temp << (length - i - 1); // shifts to convert array into binary binary_format
                                            // e.g. {1,0,1,0} => 1010
        }//for
        return id as usize;
    }//compute_index

    // key should be 1-1 mapping to self memory array
    #[inline(always)]
    fn key2index(&self, key: &[u8]) -> (usize, usize) {
        let mut current_index = self.compute_index(key);
        let mut key_start = 0;
        let mut depth = 0;
        while self.memory.len() > current_index && self.memory[current_index].is_some() {
//            println!("comp_index {} ci {} {}", self.compute_index(&key[key_start..]), current_index, self.memory.len());
            match &self.memory[current_index] {
                Some(a) => {
                    match a.children_offset {
                        Some(b) => {
                            key_start += self.key_segment_size;
                            depth += 1;
                            current_index = b + self.compute_index(&key[key_start..]);
                        }//Some(b)
                        None => break,
                    }//match a.children_offset
                }//Some(a)
                None => break,
            }//match &self.memory[current_index]
        }//while
        (current_index, depth)
    }//key2index

    // insert the entry to hash trie
    pub fn insert(&mut self, value: T, key: &[u8]) {
        let index_depth_pair = self.key2index(key); // (current_index, depth)
//        println!("debug {} {}", index_depth_pair, self.memory.len());
        if index_depth_pair.0 >= self.memory.len() {
            let push_amount = index_depth_pair.0 - self.memory.len() + 1;
            for _ in 0..push_amount {
                self.memory.push(None);
            }//for
        }//if
        if self.memory[index_depth_pair.0].is_some() {
            assert!(false);
        }//if
        self.memory[index_depth_pair.0] = Some(SubTrie {
            data: Some(value),
            depth: index_depth_pair.1,
            children_offset: None,
        });// self.memory
    }//insert

    // return true if the key entry exists
    #[inline(always)]
    pub fn contain(&self, key: &[u8]) -> bool {
        let index_depth_pair = self.key2index(key); // (current_index, depth)
        if self.memory.len() <= index_depth_pair.0 {
            return false; // can't contain the key entry if the key index is greater than the length of memory
        }//if
        match &self.memory[index_depth_pair.0] {
            Some(_) => {
                true //if there's something there, we have an entry
            }
            None => false, //otherwise, we don't have an entry
        }//match
    }//contain

    // return the value in the given key and wrap it with an Option
    #[inline(always)]
    pub fn get(&self, key: &[u8]) -> Option<T> {
        let index_depth_pair = self.key2index(key); // (current_index, depth)
        if self.memory.len() <= index_depth_pair.0 {
            return None; // can't return anything if we're out of memory bounds
        }//if
        match &self.memory[index_depth_pair.0] {
            Some(a) => {
                a.data // return the data at the given index
            }
            None => None, // if there's nothing at the given index, return None
        }//match
    }//get
}//impl ContiguousTrie

// TODO should change this to key_length+2, which is {:0key_length+2b}
#[macro_export]
macro_rules! binary_format {
    ($x:expr) => {
        format!("{:#034b}", $x)
    };
}//binary_format


fn main() {
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

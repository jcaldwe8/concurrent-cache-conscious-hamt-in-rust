/// Concurrent Cache Conscious Hash Trie using Mutex Lock

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
pub struct MutexContiguousTrie<T: TrieData> {
    memory: Arc<Mutex<Vec<Option<SubTrie<T>>>>>,
    key_length: usize,
    key_segment_size: usize,
}//struct MutexContiguousTrie


#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SubTrie<T: TrieData> {
    pub data: Option<T>,
    depth: usize,
    children_offset: Option<usize>,    // the start position in allocator that place the array in hash trie
}//struct SubTrie

// Contiguous store all the nodes contiguous with the sequential order of key
impl<T: TrieData> MutexContiguousTrie<T> {
    //constructor
    // key_length: length of the key
    // key_segment_size: length of a key segment (a key_group)
    pub fn new(key_length: usize, key_segment_size: usize) -> Self {
        // key_length needs to be multiple of key_segment_size
        assert_eq!(key_length % key_segment_size, 0);

        //memory is a vector that contains SubTries or None, with a Mutex lock
        let memory: Arc<Mutex<Vec<Option<SubTrie<T>>>>>;
        // init with all nodes that is not leaf
        // length = summation of KEY_LEN^1 to KEY_LEN^(KEY_LEN/KEY_GROUP-1)
        {//new block
            let mut nodes_length = 0;
            let array_length = usize::pow(2, key_segment_size as u32);
            let mut multitude = array_length;
            for _ in 0..(key_length / key_segment_size - 1) { //for the number of segments
                nodes_length += multitude;
                multitude *= array_length;
            }//for
//            println!("nl {}", nodes_length);
            //total memory needed for this KEY_LEN and KEY_GROUP
            memory = Arc::new(Mutex::new(Vec::with_capacity(nodes_length)));

            let mut this = memory.lock().unwrap(); //lock during access
            for i in 0..nodes_length {
                (*this).push(Some(SubTrie { //add a SubTrie in each index of the memory vector
                    data: None,
//                    depth: get_depth(key_segment_size as usize, i),
                    depth: 0,
                    children_offset: Some((i + 1) * array_length as usize),
                }));
//                println!("co {} {}", i, (i + 1) * array_length as usize);
            }//for
        }//end block

        //return this struct
        MutexContiguousTrie {
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
            id += temp << (length - i - 1);
        }
        return id as usize;
    }

    // key should be 1-1 mapping to self memory array
    #[inline(always)]
    fn key2index(&self, key: &[u8]) -> usize {
        let mut current_index = self.compute_index(key);
        let mut key_start = 0;
        let mut this = self.memory.lock().unwrap(); //lock during access
        while (*this).len() > current_index && (*this)[current_index].is_some() {
//            println!("comp_index {} ci {} {}", self.compute_index(&key[key_start..]), current_index, self.memory.len());
            match &(*this)[current_index] {
                Some(a) => {
                    match a.children_offset {
                        Some(b) => {
                            key_start += self.key_segment_size;
                            current_index = b + self.compute_index(&key[key_start..]);
                        }//Some(b)
                        None => break,
                    }//match a.children_offset
                }//Some(a)
                None => break,
            }//match &(*this)[current_index]
        }//while
        current_index
    }//key2index

    //insert the value into the Trie
    pub fn insert(&self, value: T, key: &[u8]) {
        let current_index = self.key2index(key);
        let mut this = self.memory.lock().unwrap(); //lock during access
//        println!("debug {} {}", current_index, self.memory.len());
        if current_index >= (*this).len() {
            let push_amount = current_index - (*this).len() + 1;
            for _ in 0..push_amount {
                (*this).push(None);
            }//for
        }//if
        if (*this)[current_index].is_some() {
            assert!(false);
        }//if
        (*this)[current_index] = Some(SubTrie {
            data: Some(value),
//            depth: get_depth(self.key_length, current_index),
            depth: 0,
            children_offset: None,
        });
    }//insert

    //return true if the key entry exists
    #[inline(always)]
    pub fn contain(&self, key: &[u8]) -> bool {
        let current_index = self.key2index(key);
        let mut this = self.memory.lock().unwrap(); //lock during access
        if (*this).len() <= current_index {
            return false; // can't contain the key entry if the key index is greater than the length of memory
        }//if
        match &(*this)[current_index] {
            Some(_) => {
                true //if there's something there, we have an entry
            }
            None => false, //otherwise, we don't have an entry
        }//match
    }//contain

    // return the value in the given key and wrap it with an Option
    #[inline(always)]
    pub fn get(&self, key: &[u8]) -> Option<T> {
        let current_index = self.key2index(key);
        let mut this = self.memory.lock().unwrap(); //lock during access
        if (*this).len() <= current_index {
            return None; // can't return anything if we're out of memory bounds
        }
        match &(*this)[current_index] {
            Some(a) => {
                a.data // return the data at the given index
            }
            None => None, // if there's nothing at the given index, return None
        }//match
    }//get
}//impl MutexContiguousTrie

// TODO should change this to key_length+2, which is {:0key_length+2b}
#[macro_use]
macro_rules! binary_format {
    ($x:expr) => {
        format!("{:#034b}", $x)
    };
}//binary_format

fn main() {
    let trie = Arc::new(MutexContiguousTrie::<usize>::new(32, 8));
    for t_id in 0..4 {
		let trie = trie.clone();
        let begin = t_id * 25000;
        let end = (t_id + 1) * 25000;
        thread::spawn(move || {
            for i in begin..end {
                let str = binary_format!(i);
                let arr = str.to_owned().into_bytes();
				trie.insert(i, &arr[2..]);
            }
        });
    }

    for t_id in 0..4 {
        let thread_trie = trie.clone();
        let begin = t_id * 25000;
        let end = (t_id + 1) * 25000;
        thread::spawn(move || {
            for i in begin..end {
                let str = binary_format!(i);
                let arr = str.to_owned().into_bytes();
                assert_eq!(thread_trie.get(&arr[2..]).unwrap(), i);
            }
        });
    }
}

/// The very basic hash trie implementation
/// This file is only for learning how to implement hash trie in Rust

pub trait TrieData: Clone + Copy + Eq + PartialEq {}

impl<T> TrieData for T where T: Clone + Copy + Eq + PartialEq {}

// Trie node
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Trie<T: TrieData> {
    pub data: Option<T>, // data stored in node
    depth: u32, // depth of node
    children: Vec<Option<Box<Trie<T>>>>, // this node's children
}// struct Trie

// the index status can have three possible values
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum IndexStatus {
    FullMatch,
    StartingMatch,
    NoMatch,
}//enum IndexStatus

const KEY_LEN: usize = 16; //length of key array
const KEY_GROUP: usize = 4; //length of a key group (16 distinct keys)

// index is the sum of binary in a group
// key: array representing binary
fn compute_index(key: &[u8]) -> usize {
    let mut id = 0;
    // set length to be in {0..=4}
    let length = if key.len() > KEY_GROUP { KEY_GROUP } else { key.len() };
    for i in 0..length { //convert binary to base 10
        let temp = key[i] as usize - '0' as usize;
        id += temp << i; //convert each binary digit into its base 10 equivalent
    }//for

    return id as usize;
}//compute_index

// implementation of Trie struct
impl<T: TrieData> Trie<T> {
    //constructor
    pub fn new() -> Self {
        let mut children = Vec::with_capacity(KEY_LEN);
        for i in 0..KEY_LEN {
            children.push(None);
        }//for
        Trie { data: None, depth: 0, children }
    }//constructor

    //depth: return depth of the Trie
    pub fn depth(&self) -> u32 {
        self.depth
    }//depth

    // insert a generic value by a key, and the key should be in binary format
    // value: data to be stored
    // key: binary stored in an array
    pub fn insert(&mut self, value: T, key: &[u8]) -> u32 {
        if key.len() == 0 { // when we've found the key
            self.data = Some(value);
            return 1;
        } else {
            let index = compute_index(key);

            // if the trie has not been created, then create one
            if self.children[index].is_none() {
                // println!("create subtree");
                self.children[index] = Some(Box::new(Trie::new()));
            }//if
            let value = match key.len() {
                n if n >= KEY_GROUP => {
                    //traverse the tree and insert nodes as needed
                    self.children[index].as_mut().map(|ref mut a| a.insert(value, &key[KEY_GROUP..])).unwrap_or(0)
                }
                _ => 9999,  // TODO value should be Option
            }; //set value
            self.depth += value;
            return value;
        }//if-else
    }//insert

    // get value from key
    // key: binary stored in an array
    pub fn get(&self, key: &[u8]) -> Option<T> {
        let result = self.get_sub_trie(key);

        match result {
            Some(trie) => match trie.data { // do we have a trie?
                Some(data) => return Some(data), // do we have data?
                _ => return None, // no data
            }//match
            _ => return None, // no trie
        }//match
    }//get

    // return true if the key exists, otherwise, return false
    // key: binary stored in an array
    pub fn contain(&self, key: &[u8]) -> bool {
        let trie_op = self.get_sub_trie(key);
        match trie_op {
            Some(trie) => { // do we have a trie?
                if trie.data == None {
                    return false; // no data for this key
                } else {
                    return true; // we do have data, so the key exists
                }//if-else
            }//Some(trie)
            _ => return false, // no trie => no data => no key
        }//match
    }//contain

    // index_base: return the status of the specified index
    pub fn index_base(&self, key: &[u8]) -> IndexStatus {
        if key.len() == 0 { // once we've found the key
            self.data.map(|_| IndexStatus::FullMatch).unwrap_or(IndexStatus::StartingMatch)
        } else { // traverse the trie to determine if a trie-node exists at the end of the key
            let index = compute_index(key);
            self.children[index].as_ref().map(|ref a| a.index_base(&key[KEY_GROUP..])).unwrap_or(IndexStatus::NoMatch)
        }// if-else
    }// index_base

    // get_sub_trie: return a trie at the specified key
    pub fn get_sub_trie<'a>(&'a self, key: &[u8]) -> Option<&'a Trie<T>> {
        let index = compute_index(key);
        match key.len() { //traverse the trie
            n if n >= KEY_GROUP => self.children[index].as_ref().and_then(|ref a| a.get_sub_trie(&key[KEY_GROUP..])),
            _ => Some(&self),
        }//match
    }//get_sub_trie

    // TODO delete the data in the trie found by the key
    pub fn delete_key(&mut self, key: &[u8]) {
        if key.len() == 0 {
            self.data = None; // when we've found the key, set data to None
        } else {
            let index = compute_index(key);

            if index >= KEY_GROUP { //traverse the trie
                self.children[index].as_mut().map(|ref mut a| a.delete_key(&key[KEY_GROUP..]));
            }//if
        }//if-else
    }//delete_key
}//impl Trie

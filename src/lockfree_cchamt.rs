use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::sync::atomic::{AtomicPtr, Ordering, AtomicU32};
use std::option::Option;
use std::ptr::null_mut;
use allocator::Allocator;
use std::thread;

pub trait TrieData: Clone + Copy + Eq + PartialEq {}

impl<T> TrieData for T where T: Clone + Copy + Eq + PartialEq {}

pub trait TrieKey: Clone + Copy + Eq + PartialEq + Hash {}

impl<T> TrieKey for T where T: Clone + Copy + Eq + PartialEq + Hash {}

type ANode<K, V> = Vec<AtomicPtr<Node<K, V>>>;

//nodes can be of 7 distinct types:
//#[derive(Clone)]
enum Node<K, V> {
    SNode { //stores data
        hash: u64,
        key: K,
        val: V,
        txn: AtomicPtr<Node<K, V>>,
    },
    ANode(ANode<K, V>), //array node
    NoTxn, //no transaction (only valid for txn field in SNode; see above)
    FSNode, //indicates that an SNode is frozen
    FVNode, //indicates that an empty array node is frozen
    FNode { //indicates that an ANode is frozen
        frozen: AtomicPtr<Node<K, V>>
    },
    ENode { //expansion node, represents in-process expansion of an ANode
        parent: AtomicPtr<Node<K, V>>,
        parentpos: u8,
        narrow: AtomicPtr<Node<K, V>>,
        hash: u64,
        level: u8,
        wide: AtomicPtr<Node<K, V>>,
    },
}//enum Node

//impl <K, V> Clone for AtomicPtr<Node<K, V>> {
//    fn clone(&self) -> AtomicPtr<Node<K, V>>{

//    }//clone
//}//Clone

use std::mem::discriminant;
// note_type_eq: determine if two nodes have the same enum type
//  From stackoverflow: Compare enums only by variant, not value
fn node_type_eq<K, V>(n: Node<K, V>, m: &Node<K, V>) -> bool {
    discriminant(&n) == discriminant(m)
}//node_type_eq

// hash: return hashcode for input object
fn hash<T>(obj: T) -> u64
    where
        T: Hash {
    let mut hasher = DefaultHasher::new();
    obj.hash(&mut hasher); //add obj to hasher
    hasher.finish() //return hashcode for included items(i.e., obj)
}//hash

//maximum # of allowable misses
const MAX_MISSES: u32 = 2048;   // play with this

//struct for CacheLevel
struct CacheLevel<K: TrieKey, V: TrieData> {
    parent: AtomicPtr<CacheLevel<K, V>>, //parent CacheLevel
    pub nodes: Vec<AtomicPtr<Node<K, V>>>, //children
    pub misses: Vec<AtomicU32>, //number of misses
}//struct CacheLevel

//implementation of CacheLevel struct
impl<K: TrieKey, V: TrieData> CacheLevel<K, V> {
    //constructor
    // level: level of the trie
    // tfact:
    // ncpu: number of cpu's
    pub fn new(level: u8, tfact: f64, ncpu: u8) -> Self {
        // length for nodes
        let len = 1 << level; //left shift, effectively multiply by 2^level
        let mut nodes = Vec::with_capacity(len);

        for i in 0..len { //initialize nodes
            nodes[i] = AtomicPtr::new(null_mut());
        }//for

        // length for misses
        let len = (tfact * ncpu as f64) as usize;
        let mut misses = Vec::with_capacity(len);

        for i in 0..len { //initialize number of misses for each entry
            misses[i] = AtomicU32::new(0);
        }//for

        //return this struct
        CacheLevel {
            parent: AtomicPtr::new(null_mut()),
            nodes: nodes,
            misses: misses,
        }// CacheLevel struct
    }//constructor

    // parent: return the parent CacheLevel
    pub fn parent(&self) -> Option<&mut CacheLevel<K, V>> {
        let p = self.parent.load(Ordering::Relaxed);
        if p.is_null() { None } else { Some(unsafe { &mut *p }) }
    }//parent

}//impl CacheLevel

//structure for Cache
struct Cache<K: TrieKey, V: TrieData> {
    level: AtomicPtr<CacheLevel<K, V>>
}//struct Cache

//implementation of Cache struct
impl<K: TrieKey, V: TrieData> Cache<K, V> {
    //constructor
    pub fn new() -> Self {
        // a Cache initially consists of a single CacheLevel
        Cache { //return this struct
            level: AtomicPtr::new(null_mut()) // CacheLevel::new(0, 0.3, 8),
        }//Cache struct
    }//constructor
}//impl Cache

//structure for LockfreeTrie; public
pub struct LockfreeTrie<K: TrieKey, V: TrieData> {
    root: AtomicPtr<Node<K, V>>, //root node
    mem: Allocator<Node<K, V>>, //memory allocator
    cache: AtomicPtr<CacheLevel<K, V>>, //essentially a Cache struct
}//struct Cache

// makeanode: return an ANode with length len and empty elements
fn makeanode<K, V>(len: usize) -> ANode<K, V> {
    let mut a: ANode<K, V> = Vec::with_capacity(len);
    for i in 0..len {
        a.push(AtomicPtr::new(null_mut()));
    }//for
    a //return the array node
} //makeanode

fn get_ary_length<K, V>(cur: &Node<K, V>) -> usize {
    let mut len: usize = 1;
    if let Node::ANode(ref cur2) = cur {
        len = cur2.len();
    }
    len
}

fn is_node_null<K, V>(node: &mut Node<K, V>) -> bool {
    match node {
        SNode => { false },
        ANode => { false },
        NoTxn => { false },
        FSNode => { false },
        FVNode => { false },
        ENode => { false },
        _ => { true }
    }
}

fn is_oldptr_null<K, V>(cur: &Node<K, V>, pos: usize) -> bool {
    let mut is_null: bool = true;
    if let Node::ANode(ref cur2) = cur {
        let old = &cur2[pos];
        let oldptr = old.load(Ordering::Relaxed);
        let oldref = unsafe { &mut *oldptr };
        //is_null = oldref.is_null();
        is_null = is_node_null(oldref);
    }
    is_null
}

fn get_old<K, V>(cur: &Node<K, V>, pos: usize) -> &AtomicPtr<Node<K, V>> {
    let old: &AtomicPtr<Node<K, V>>;
    if let Node::ANode(ref cur2) = cur {
        old = &cur2[pos];
    } else {
        panic!("Shouldn't be here");
    }
    old
}

fn get_oldptr<K, V>(cur: &Node<K, V>, pos: usize) -> *mut Node<K, V> {
    let oldptr: *mut Node<K, V>;
    if let Node::ANode(ref cur2) = cur {
        let old = &cur2[pos];
        oldptr = old.load(Ordering::Relaxed);
    } else {
        panic!("Shouldn't be here");
    }
    oldptr
}

fn get_oldref<K, V>(cur: &Node<K, V>, pos: usize) -> &mut Node<K, V> {
    let oldref: &mut Node<K, V>;
    if let Node::ANode(ref cur2) = cur {
        let old = &cur2[pos];
        let oldptr = old.load(Ordering::Relaxed);
        oldref = unsafe { &mut *oldptr };
    } else {
        panic!("Shouldn't be here");
    }
    oldref
}

fn get_prev2aptr<K, V>(prev: &Node<K, V>, ppos: usize) -> &AtomicPtr<Node<K, V>> {
    let prev2aptr: &AtomicPtr<Node<K, V>>;
    if let Node::ANode(ref prev2) = prev {
        prev2aptr = &prev2[ppos];
    } else {
        panic!("Shouldn't be here")
    }
    prev2aptr
}

fn get_narrowptr<K, V>(enode: &Node<K, V>) -> *mut Node<K, V> {
    let narrowptr: *mut Node<K, V>;
    if let Node::ENode { ref parent, parentpos, ref narrow, level, wide: ref _wide, .. } = enode {
        narrowptr = narrow.load(Ordering::Relaxed);
    } else {
        panic!("Shouldn't be here");
    }
    narrowptr
}

fn get_enode_level<K, V>(enode: &Node<K, V>) -> u8 {
    let lev: u8;
    if let Node::ENode { ref parent, parentpos, ref narrow, level, wide: ref _wide, .. } = enode {
        lev = *level;
    } else {
        panic!("Shouldn't be here");
    }
    lev
}

fn get_enode__wide<K, V>(enode: &Node<K, V>) -> &AtomicPtr<Node<K, V>>{
    let _ret_wide: &AtomicPtr<Node<K, V>>;
    if let Node::ENode { ref parent, parentpos, ref narrow, level, wide: ref _wide, .. } = enode {
        _ret_wide = _wide;
    } else {
        panic!("Shouldn't be here");
    }
    _ret_wide
}

fn get_enode_parentref<K, V>(enode: &Node<K, V>) -> &Node<K, V> {
    let parentref: &Node<K, V>;
    if let Node::ENode { ref parent, parentpos, ref narrow, level, wide: ref _wide, .. } = enode {
        parentref = unsafe { &*parent.load(Ordering::Relaxed) };
    } else {
        panic!("Shouldn't be here");
    }
    parentref
}

fn get_enode_parentpos<K, V>(enode: &Node<K, V>) -> &u8 {
    let parentposi: &u8;
    if let Node::ENode { ref parent, parentpos, ref narrow, level, wide: ref _wide, .. } = enode {
        parentposi = parentpos;
    } else {
        panic!("Shouldn't be here");
    }
    parentposi
}

/**
 * TODO: fix memory leaks and use atomic_ref or crossbeam crates
 */

//implementation of LockfreeTrie struct
impl<K: TrieKey, V: TrieData> LockfreeTrie<K, V> {
    //constructor
    pub fn new() -> Self {
        //let mem = Allocator::new(1000000000);
        let mem = Allocator::new(100000000); //test
        LockfreeTrie {//return this struct
            root: AtomicPtr::new(mem.alloc(Node::ANode(makeanode(16)))),
            mem: mem,
            cache: AtomicPtr::new(null_mut()),
        }//return struct
    }//constructor

    //_freeze: lock the elements of an ANode until they can be safely unlocked
    // nnode: must be an ANode, or method will panic!
    fn _freeze(mem: &Allocator<Node<K, V>>, nnode: &mut Node<K, V>) -> () {
         //let cur be a reference to the items in nnode
         //only continue if the items in nnode match those found in an ANode
        if let Node::ANode(ref cur) = nnode {
            let mut i = 0;
            while i < cur.len() { //go through the entire array
                let node = &cur[i]; //node at position i in array
                let nodeptr = node.load(Ordering::Relaxed); //ptr to node
                let noderef = unsafe { &mut *nodeptr }; //ref to node

                i += 1; //increase to move forward; future decreases act as lock
                if nodeptr.is_null() {
                    //update nodeptr to mem.alloc(Node::FVNode)
                    if node.compare_and_swap(nodeptr, mem.alloc(Node::FVNode), Ordering::Relaxed) != nodeptr {
                        i -= 1; //lock
                    }//if
                } else if let Node::SNode { ref txn, .. } = noderef { //if the node is an SNode
                    let txnptr = txn.load(Ordering::Relaxed);
                    let txnref = unsafe { &mut *txnptr };
                    if let Node::NoTxn = txnref { //if the txn is set to NoTxn
                        //update txnptr to mem.alloc(Node::FSNode)
                        if txn.compare_and_swap(txnptr, mem.alloc(Node::FSNode), Ordering::Relaxed) != txnptr {
                            i -= 1; //lock
                        }//if
                    } else if let Node::FSNode = txnref {} else { //if txnref is a frozen SNode
                        //update nodeptr to txnptr
                        node.compare_and_swap(nodeptr, txnptr, Ordering::Relaxed);
                        i -= 1; //lock
                    }//if-else
                //} else if let Node::ANode(ref an) = noderef { //if the node is an ANode
                } else if node_type_eq(Node::ANode(makeanode(4)), noderef) { //if the node is an ANode
                    //declare a frozen ANode
                    let fnode = mem.alloc(Node::FNode { frozen: AtomicPtr::new(noderef) });
                    //update nodeptr to fnode
                    node.compare_and_swap(nodeptr, fnode, Ordering::Relaxed);
                    i -= 1; //lock
                } else if let Node::FNode { ref frozen } = noderef { //if the node is an FNode
                    LockfreeTrie::_freeze(mem, unsafe { &mut *frozen.load(Ordering::Relaxed) });
                } else if let Node::ENode { .. } = noderef { //if the node is an ENode
                    //complete the expansion of the node before proceeding
                    LockfreeTrie::_complete_expansion(mem, noderef);
                    i -= 1; //lock
                }//if-else
            }//while
        } else { //if we don't have an ANode for input
            // this has never happened once, but just to be sure...
            panic!("CORRUPTION: nnode is not an ANode")
        }//if-else
    }//_freeze

    //_copy: recursively copy elements of a narrow array (4 elements) into a wide array (16 elements)
    fn _copy(mem: &Allocator<Node<K, V>>, an: &ANode<K, V>, wide: &mut Node<K, V>, lev: u64) -> () {
        for node in an { //for every element in the ANode
            match unsafe { &*node.load(Ordering::Relaxed) } { //match the entry
                Node::FNode { ref frozen } => { //if we have an FNode, make a ref to the frozen ANode
                    //make a reference ptr to the ANode
                    let frzref = unsafe { &*frozen.load(Ordering::Relaxed) };
                    if let Node::ANode(ref an2) = frzref {
                        LockfreeTrie::_copy(mem, an2, wide, lev); //recursively copy into this array
                    } else { //if the node somehow isn't an ANode
                        // this has never happened once, but just to be sure...
                        panic!("CORRUPTION: FNode contains non-ANode")
                    }//if-else
                } //FNode
                Node::SNode { hash, key, val, txn } => { //if we have an SNode, copy data indo wide array
                    LockfreeTrie::_insert(mem, *key, *val, *hash, lev as u8, wide, None);
                }//SNode
                _ => { /* ignore; not an F or S Node */ }
            }//match
        }//for
    }//_copy

    //_complete_expansion: complete the expansion of an ENode
    fn _complete_expansion(mem: &Allocator<Node<K, V>>, enode: &mut Node<K, V>) -> () {
        //if we don't have an ENode, panic!
        //make refs to parent, narrow, and wide
        //parentpos and level don't need refs, because they're primitive
        //if let Node::ENode { ref parent, parentpos, ref narrow, level, wide: ref mut _wide, .. } = enode {
        if node_type_eq(Node::ENode {
            parent: AtomicPtr::new(mem.alloc(Node::ANode(makeanode(4)))), //parent array node
            parentpos: 1 as u8, //position in parent array node
            narrow: AtomicPtr::new(mem.alloc(Node::ANode(makeanode(4)))), //narrow version of enode (currently populated)
            hash: 1 as u64,
            level: 1 as u8,
            wide: AtomicPtr::new(null_mut()), //wide version of enode (not yet populated)
        }, enode) {
            //let narrowptr = narrow.load(Ordering::Relaxed); //ptr to narrow array
            let narrowptr = get_narrowptr(enode);
            LockfreeTrie::_freeze(mem, unsafe { &mut *narrowptr });//freeze narrow (make sure we can proceed)
            let mut widenode = mem.alloc(Node::ANode(makeanode(16))); //make an ANode with 16 elements
            let level = get_enode_level(enode);
            if let Node::ANode(ref an) = unsafe { &*narrowptr } { //make ref to narrow array
                //LockfreeTrie::_copy(mem, an, unsafe { &mut *widenode }, *level as u64); //copy narrow elements into widearray
                LockfreeTrie::_copy(mem, an, unsafe { &mut *widenode }, level as u64);
            } else {
                // this has never happened once, but just to be sure...
                panic!("CORRUPTION: narrow is not an ANode")
            }//if-else
            //switch to the wide array
            //if _wide.compare_and_swap(null_mut(), widenode, Ordering::Relaxed) != null_mut() {
            if get_enode__wide(enode).compare_and_swap(null_mut(), widenode, Ordering::Relaxed) != null_mut() {
                //let _wideptr = _wide.load(Ordering::Relaxed);
                let _wideptr = get_enode__wide(enode).load(Ordering::Relaxed);
                if let Node::ANode(ref an) = unsafe { &mut *_wideptr } {
                    widenode = unsafe { &mut *_wideptr }; //set ptr to widenode
                } else {
                    // this has never happened once, but just to be sure...
                    panic!("CORRUPTION: _wide is not an ANode")
                }//if-else
            }//if
            //let parentref = unsafe { &*parent.load(Ordering::Relaxed) };
            let parentref = get_enode_parentref(enode);
            let parentpos = get_enode_parentpos(enode);
            if let Node::ANode(ref an) = parentref { //set ref to parent
                let anptr = &an[*parentpos as usize];
                anptr.compare_and_swap(enode, widenode, Ordering::Relaxed);
            } else {
                // this has never happened once, but just to be sure...
                panic!("CORRUPTION: parent is not an ANode")
            }//if-else
        } else {
            // this has never happened once, but just to be sure...
            panic!("CORRUPTION: enode is not an ENode")
        }//if-else
    }//_complete_expansion

    //_create_anode: if we already have data at an index,
    //               make an ANode with length 4 and hash both nodes into it
    // old: SNode already hashed to index
    // sn: new SNode that we want to insert
    // lev: level of the trie (used to determine which bits to use)
    fn _create_anode(mem: &Allocator<Node<K, V>>, old: Node<K, V>, sn: Node<K, V>, lev: u8) -> ANode<K, V> {
        let mut v = makeanode(4);

        if let Node::SNode { hash: h_old, .. } = old { //ref to hash in SNode
            let old_pos = (h_old >> lev) as usize & (v.len() - 1); //only use 2 bits associated with lev
            if let Node::SNode { hash: h_sn, .. } = sn { //ref to hash in SNode
                let sn_pos = (h_sn >> lev) as usize & (v.len() - 1); //only use 2 bits associated with lev
                if old_pos == sn_pos {
                    v[old_pos] = AtomicPtr::new(mem.alloc(Node::ANode(LockfreeTrie::_create_anode(mem, old, sn, lev + 4))));
                } else {
                    v[old_pos] = AtomicPtr::new(mem.alloc(old));
                    v[sn_pos] = AtomicPtr::new(mem.alloc(sn));
                }//if-else
            } else {
                // this has never happened once, but just to be sure...
                panic!("CORRUPTION: expected SNode");
            }
        } else {//if-else
            // this has never happened once, but just to be sure...
            panic!("CORRUPTION: expected SNode");
        }//if-else
        return v;
    }//_create_anode



    //_insert: insert a node into the hamt with value V at key K with allocator mem
    fn _insert(mem: &Allocator<Node<K, V>>, //memory allocator
               key: K, val: V, h: u64, lev: u8, //hash key, value, code, and level
               cur: &mut Node<K, V>, //current node (ANode)
               prev: Option<&mut Node<K, V>>) -> bool { //previous node

        if node_type_eq(Node::ANode(makeanode(4)), cur) { //if the node is an ANode
        //if let Node::ANode(ref mut cur2) = cur { //ref to ANode in enum of ANode

            let pos = (h >> lev) as usize & (get_ary_length(cur) - 1);
            //let pos = (h >> lev) as usize & (cur2.len() - 1); //index
            //let old = &cur2[pos]; //value at pos
            //let oldptr = old.load(Ordering::Relaxed);
            //let oldref = unsafe { &mut *oldptr };

            //if oldptr.is_null() { //if there isn't a node at the current pos
            if is_oldptr_null(cur, pos) {
            //define an SNode
                let sn = mem.alloc(Node::SNode {
                    hash: h,
                    key: key,
                    val: val,
                    txn: AtomicPtr::new(mem.alloc(Node::NoTxn)),
                });
                //update oldptr
                //if old.compare_and_swap(oldptr, sn, Ordering::Relaxed) == oldptr {
                if get_old(cur, pos).compare_and_swap(get_oldptr(cur, pos), sn, Ordering::Relaxed) == get_oldptr(cur, pos) {
                    true
                } else {
                    LockfreeTrie::_insert(mem, key, val, h, lev, cur, prev)
                }//if-else
            //} else if let Node::ANode(ref mut an) = oldref { //if we have an ANode
            } else if node_type_eq(Node::ANode(makeanode(4)), get_oldref(cur, pos)) { //if the node is an ANode
                LockfreeTrie::_insert(mem, key, val, h, lev + 4, get_oldref(cur, pos), Some(cur))
            } else if let Node::SNode { hash: _hash, key: _key, val: _val, ref mut txn } = get_oldref(cur, pos) { //if we have an SNode
                let txnptr = txn.load(Ordering::Relaxed);
                let txnref = unsafe { &*txnptr };

                if let Node::NoTxn = txnref { //if the SNode has NoTxn
                    if *_key == key { //if the insert key and key at this index match
                        let sn = mem.alloc(Node::SNode { //make a new SNode
                            hash: h,
                            key: key,
                            val: val,
                            txn: AtomicPtr::new(mem.alloc(Node::NoTxn)),
                        });
                        if txn.compare_and_swap(txnptr, sn, Ordering::Relaxed) == txnptr {
                            get_old(cur, pos).compare_and_swap(get_oldptr(cur, pos), sn, Ordering::Relaxed);
                            true
                        } else {
                            LockfreeTrie::_insert(mem, key, val, h, lev, cur, prev)
                        }
                    } else if get_ary_length(cur) == 4 { //if we have a narrow array (might need to expand)
                        if let Some(prevref) = prev {
                            //if let Node::ANode(ref mut prev2) = prevref {
                            if node_type_eq(Node::ANode(makeanode(4)), prevref) {
                                //let ppos = (h >> (lev - 4)) as usize & (prev2.len() - 1);
                                let ppos = (h >> (lev - 4)) as usize & (get_ary_length(prevref) - 1);
                                //let prev2aptr = &prev2[ppos];
                                let en = mem.alloc(Node::ENode {
                                    parent: AtomicPtr::new(prevref),
                                    parentpos: ppos as u8,
                                    narrow: AtomicPtr::new(cur),
                                    hash: h,
                                    level: lev,
                                    wide: AtomicPtr::new(null_mut()),
                                });
                                //if prev2aptr.compare_and_swap(cur, en, Ordering::Relaxed) == cur {
                                if get_prev2aptr(prevref, ppos).compare_and_swap(cur, en, Ordering::Relaxed) == cur {
                                    LockfreeTrie::_complete_expansion(mem, unsafe { &mut *en });
                                    if let Node::ENode { ref wide, .. } = unsafe { &mut *en } {
                                        let wideref = unsafe { &mut *wide.load(Ordering::Relaxed) };
                                        LockfreeTrie::_insert(mem, key, val, h, lev, wideref, Some(prevref))
                                    } else {
                                        // this has never happened once, but just to be sure...
                                        panic!("CORRUPTION: en is not an ENode")
                                    }
                                } else {
                                    LockfreeTrie::_insert(mem, key, val, h, lev, cur, Some(prevref))
                                }
                            } else {
                                // this has never happened once, but just to be sure...
                                panic!("CORRUPTION: prevref is not an ANode")
                            }
                        } else {
                            // this has never happened once, but just to be sure...
                            panic!("ERROR: prev is None")
                        }
                    } else { //if we don't have an array, create one
                        let an = mem.alloc(Node::ANode(LockfreeTrie::_create_anode(mem,
                                                                                   Node::SNode {
                                                                                       hash: *_hash,
                                                                                       key: *_key,
                                                                                       val: *_val,
                                                                                       txn: AtomicPtr::new(mem.alloc(Node::NoTxn)),
                                                                                   },
                                                                                   Node::SNode {
                                                                                       hash: h,
                                                                                       key: key,
                                                                                       val: val,
                                                                                       txn: AtomicPtr::new(mem.alloc(Node::NoTxn)),
                                                                                   }, lev + 4)));
                        if txn.compare_and_swap(txnptr, an, Ordering::Relaxed) == txnptr {
                            get_old(cur, pos).compare_and_swap(get_oldptr(cur, pos), an, Ordering::Relaxed);
                            true
                        } else {
                            LockfreeTrie::_insert(mem, key, val, h, lev, cur, prev)
                        }
                    }
                } else if let Node::FSNode = txnref {
                    false
                } else {
                    get_old(cur, pos).compare_and_swap(get_oldptr(cur, pos), txnptr, Ordering::Relaxed);
                    LockfreeTrie::_insert(mem, key, val, h, lev, cur, prev)
                }
            } else { //otherwise
                if let Node::ENode { .. } = get_oldref(cur, pos) {
                    LockfreeTrie::_complete_expansion(mem, get_oldref(cur, pos));
                }
                false
            }
        } else {
            // this has never happened once, but just to be sure...
            panic!("CORRUPTION: curref is not an ANode")
        }
    }//_insert

    //insert: call the _insert function
    pub fn insert(&mut self, key: K, val: V) -> bool {
        LockfreeTrie::_insert(&mut self.mem, key, val, hash(key), 0, unsafe { &mut *self.root.load(Ordering::Relaxed) }, None)
            || self.insert(key, val)
    }//insert

    //_inhabit:
    fn _inhabit<'a>(&'a self,
                    cache: Option<&'a CacheLevel<K, V>>, //CacheLevel
                    nv: *mut Node<K, V>, //mutable Node
                    hash: u64, //hashcode
                    lev: u8) -> () { //level of trie

        if let Some(level) = cache { //if cache is Option==Some, ref its level
            let length = level.nodes.capacity();
            let cache_level = (length - 1).trailing_zeros();
            if cache_level == lev.into() { //if we're on the same level of the cache and trie
                //Note: CacheLevel.nodes has power of 2 capacity
                let pos = hash as usize & (length - 1);
                (&level.nodes[pos]).store(nv, Ordering::Relaxed);
            }//if
        } else { //if cache is None
            if lev >= 12 {
                let clevel = Box::into_raw(box CacheLevel::new(lev, 0.3, 8));
                let levptr = self.cache.load(Ordering::Relaxed);
                let oldptr = self.cache.compare_and_swap(levptr, clevel, Ordering::Relaxed);

                if !oldptr.is_null() {
                    let _b = unsafe { Box::from_raw(oldptr) };
                }//if

                self._inhabit(Some(unsafe { &*clevel }), nv, hash, lev);
            }//if
        }//if-else
    }//_inhabit

    //_record_miss: record a cache miss and adjust the cache size if needed
    fn _record_miss(&self) -> () {
        let mut counter_id: u64 = 0; //initialize
        let mut count: u32 = 0; //initialize
        let levptr = self.cache.load(Ordering::Relaxed);
        if !levptr.is_null() {
            let cn = unsafe { &*levptr };
            {//new block
                //generate id from thread and capacity of misses
                counter_id = hash(thread::current().id()) % cn.misses.capacity() as u64;
                //get # of misses from id
                count = cn.misses[counter_id as usize].load(Ordering::Relaxed);
            }//end block
            if count > MAX_MISSES { //if we have too many misses
                (&cn.misses[counter_id as usize]).store(0, Ordering::Relaxed); //reset misses to 0
                self._sample_and_adjust(Some(cn)); //adjust the cache accordingly
            } else {
                //otherwise, add one to the recorded count
                (&cn.misses[counter_id as usize]).store(count + 1, Ordering::Relaxed);
            }//if-else
        }//if
    }//_record_miss

    //_sample_and_adjust: sample the snodes and expand the level that has the most, if needed
    fn _sample_and_adjust<'a>(&'a self,
                              cache: Option<&'a CacheLevel<K, V>>) -> () {

        if let Some(level) = cache { //if cache is Option==Some, ref its level
            let histogram = self._sample_snodes_levels();
            let mut best = 0;
            for i in 0..histogram.len() { //find which level has the most snodes
                if histogram[i] > histogram[best] {
                    best = i;
                }//if
            }//for
            //prev capacity
            let prev = (level.nodes.capacity() as u64 - 1).trailing_zeros() as usize;
            if (histogram[best as usize] as f32) > histogram[prev >> 2] as f32 * 1.5 {
                self._adjust_level(best << 2);
            }//if
        }//if
    }//_sample_and_adjust

    //_adjust_level:
    fn _adjust_level(&self, level: usize) -> () {
        let clevel = Box::into_raw(box CacheLevel::new(level as u8, 0.3, 8));
        let levptr = self.cache.load(Ordering::Relaxed);
        let oldptr = self.cache.compare_and_swap(levptr, clevel, Ordering::Relaxed);

        if !oldptr.is_null() {
            let _b = unsafe { Box::from_raw(oldptr) };
        }//if
    }//_adjust_level

    //_fill_hist: fill hist with # of SNodes at each level
    fn _fill_hist(hist: &mut Vec<i32>, node: &Node<K, V>, level: u8) -> () {
        if let Node::ANode(ref an) = node {
            for v in an { //for all elements in the array
                let vptr = v.load(Ordering::Relaxed);

                if !vptr.is_null() { //if the element isn't null
                    let vref = unsafe { &*vptr };

                    if let Node::SNode { .. } = vref { //if vref refers to an SNode
                        if level as usize >= hist.capacity() { //increase the hist size if needed
                            hist.resize_default((level as usize) << 1);
                            hist[level as usize] = 0;
                        }//if
                        hist[level as usize] += 1; //add one to cache level
                    } else if let Node::ANode(_) = vref { // if its an ANode, fill hist for that level
                        LockfreeTrie::_fill_hist(hist, vref, level + 1);
                    }//if-else
                }//if
            }//for
        }//if
    }//_fill_hist

    //_sample_snodes_levels:
    fn _sample_snodes_levels(&self) -> Vec<i32> {
        let mut hist = Vec::new();

        let root = unsafe { &*self.root.load(Ordering::Relaxed) };
        LockfreeTrie::_fill_hist(&mut hist, root, 0);

        hist
    }//_sample_snodes_levels

    //_lookup:
    fn _lookup<'a>(&self, key: &K, h: u64, lev: u8, cur: &'a mut Node<K, V>,
                   cache: Option<&'a CacheLevel<K, V>>, cache_lev: Option<u8>) -> Option<&'a V> {

        //if let Node::ANode(ref cur2) = cur { //if cur is of enum type ANode, make reference to array node
        if node_type_eq(Node::ANode(makeanode(4)), cur) {
            if Some(lev) == cache_lev { //if cache_lev contains something
                self._inhabit(cache, cur, h, lev);
            }//if

            let cur2:Vec<AtomicPtr<Node<K, V>>>;
            /*
            match cur {
                Node::ANode(ref cur2) => {  }, // maybe put everything in here?
                _ => { panic!("ANode needs to contain array") }
                //None => { panic!("ANode needs to contain array") }
            }
            */
            match cur {
                Node::ANode(ref cur2) => {
                    let pos = (h >> lev) as usize & (cur2.len() - 1); //index for level
                    let oldptr = (&cur2[pos]).load(Ordering::Relaxed);
                    let oldref = unsafe { &mut *oldptr };

                    if oldptr.is_null() { //if there isn't anything at pos
                        None
                    } else if let Node::FVNode = oldref { //if oldref refs to an empty frozen array node
                        None
                    //} else if let Node::ANode(ref an) = oldref {  //if it refs to an ANode
                    } else if node_type_eq(Node::ANode(makeanode(4)), oldref) { //if the node is an ANode
                        self._lookup(key, h, lev + 4, oldref, cache, cache_lev) //look further down the trie
                    } else if let Node::SNode { key: _key, val, .. } = oldref { //if it contains data
                        if let Some(clev) = cache_lev {
                            if !(lev >= clev || lev <= clev + 4) {
                                self._record_miss();
                            }//if
                            if lev + 4 == clev {
                                self._inhabit(cache, oldptr, h, lev + 4);
                            }//if
                        }//if
                        if *_key == *key {
                            Some(val)
                        } else {
                            None
                        }//if-else
                    } else if let Node::ENode { narrow, .. } = oldref {
                        self._lookup(key, h, lev + 4, unsafe { &mut *narrow.load(Ordering::Relaxed) }, cache, cache_lev)
                    } else if let Node::FNode { frozen } = oldref {
                        self._lookup(key, h, lev + 4, unsafe { &mut *frozen.load(Ordering::Relaxed) }, cache, cache_lev)
                    } else {
                        // this has never happened once, but just to be sure...
                        panic!("CORRUPTION: oldref is not a valid node")
                    }
                }, // maybe put everything in here?
                _ => { panic!("ANode needs to contain array") }
            }
            /*
            if let Node::ANode(ref cur2) = cur {
                //do nothing
            } else {
                panic!("Shouldn't be here")
            }
            */
            /*
            //let Node::ANode(ref cur2) = cur; //moved from if-statement
            let pos = (h >> lev) as usize & (cur2.len() - 1); //index for level
            let oldptr = (&cur2[pos]).load(Ordering::Relaxed);
            let oldref = unsafe { &mut *oldptr };

            //moved up ~10 lines
            //if Some(lev) == cache_lev { //if cache_lev contains something
            //    self._inhabit(cache, cur, h, lev);
            //}//if

            //moved up into match statement (~50 lines)
            if oldptr.is_null() { //if there isn't anything at pos
                None
            } else if let Node::FVNode = oldref { //if oldref refs to an empty frozen array node
                None
            //} else if let Node::ANode(ref an) = oldref {  //if it refs to an ANode
            } else if node_type_eq(Node::ANode(makeanode(4)), oldref) { //if the node is an ANode
                self._lookup(key, h, lev + 4, oldref, cache, cache_lev) //look further down the trie
            } else if let Node::SNode { key: _key, val, .. } = oldref { //if it contains data
                if let Some(clev) = cache_lev {
                    if !(lev >= clev || lev <= clev + 4) {
                        self._record_miss();
                    }//if
                    if lev + 4 == clev {
                        self._inhabit(cache, oldptr, h, lev + 4);
                    }//if
                }//if
                if *_key == *key {
                    Some(val)
                } else {
                    None
                }//if-else
            } else if let Node::ENode { narrow, .. } = oldref {
                self._lookup(key, h, lev + 4, unsafe { &mut *narrow.load(Ordering::Relaxed) }, cache, cache_lev)
            } else if let Node::FNode { frozen } = oldref {
                self._lookup(key, h, lev + 4, unsafe { &mut *frozen.load(Ordering::Relaxed) }, cache, cache_lev)
            } else {
                // this has never happened once, but just to be sure...
                panic!("CORRUPTION: oldref is not a valid node")
            }
            */
        } else {
            // this has never happened once, but just to be sure...
            panic!("CORRUPTION: cur is not a pointer to ANode")
        }//if-else
    }//_lookup

    /**
     * implemented as fastLookup()
     */
    pub fn lookup(&self, key: &K) -> Option<&V> {
        let h = hash(key);
        let mut cache_head_ptr = self.cache.load(Ordering::Relaxed);

        if cache_head_ptr.is_null() {
            self._lookup(key, hash(key), 0, unsafe { &mut *self.root.load(Ordering::Relaxed) }, None, None)
        } else {
            let cache_head = unsafe { &*cache_head_ptr };
            let top_level = (cache_head.nodes.capacity() - 1).trailing_zeros();
            while !cache_head_ptr.is_null() {
                let cache_head = unsafe { &*cache_head_ptr };
                let pos = h & (cache_head.nodes.capacity() - 1) as u64;
                let cachee_ptr = cache_head.nodes[pos as usize].load(Ordering::Relaxed);
                let level = (cache_head.nodes.capacity() - 1).trailing_zeros();
                if !cachee_ptr.is_null() {
                    let cachee = unsafe { &*cachee_ptr };
                    if let Node::SNode { txn, key: _key, val, .. } = cachee {
                        if let Node::NoTxn = unsafe { &*txn.load(Ordering::Relaxed) } {
                            if *_key == *key {
                                return Some(val);
                            } else {
                                return None;
                            }
                        }
                    } else if let Node::ANode(ref an) = cachee {
                        let cpos = (h >> level) & (an.capacity() - 1) as u64;
                        let oldptr = an[cpos as usize].load(Ordering::Relaxed);

                        if !oldptr.is_null() {
                            if let Node::SNode { txn, .. } = unsafe { &*oldptr } {
                                if let Node::FSNode = unsafe { &*txn.load(Ordering::Relaxed) } { continue; }
                            }
                        }
                        return self._lookup(key, hash(key), 0, unsafe { &mut *self.root.load(Ordering::Relaxed) }, Some(cache_head), Some(level as u8));
                    }
                }
                cache_head_ptr = cache_head.parent.load(Ordering::Relaxed);
            }
            self._lookup(key, hash(key), 0, unsafe { &mut *self.root.load(Ordering::Relaxed) }, None, Some(top_level as u8))
        }
    }
}

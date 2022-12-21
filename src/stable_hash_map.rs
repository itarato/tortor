use std::{borrow::Borrow, collections::HashMap, hash::Hash, ops::Index};

#[derive(Debug)]
pub struct StableHashMap<K, V> {
    map: HashMap<K, V>,
    keys: Vec<K>,
}

impl<K: Eq + Hash, V> StableHashMap<K, V> {
    pub fn new() -> Self {
        StableHashMap {
            map: HashMap::new(),
            keys: vec![],
        }
    }

    pub fn iter<'a>(&'a self) -> StableHashMapIter<'a, K, V> {
        StableHashMapIter {
            stable_hash_map: self,
            index: 0,
        }
    }

    pub fn insert(&mut self, key: K, value: V)
    where
        K: Clone,
    {
        self.map.insert(key.clone(), value);
        self.keys.push(key);
    }
}

impl<K, Q: ?Sized, V> Index<&Q> for StableHashMap<K, V>
where
    K: Eq + Hash + Borrow<Q>,
    Q: Eq + Hash,
{
    type Output = V;

    fn index(&self, k: &Q) -> &Self::Output {
        self.map.get(&k).unwrap()
    }
}

pub struct StableHashMapIter<'a, K, V> {
    stable_hash_map: &'a StableHashMap<K, V>,
    index: usize,
}

impl<'a, K: Eq + Hash, V> Iterator for StableHashMapIter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.stable_hash_map.keys.len() {
            return None;
        }

        self.index += 1;

        let key = self
            .stable_hash_map
            .keys
            .get(self.index as usize - 1)
            .unwrap();
        let val = self.stable_hash_map.map.get(key).unwrap();

        Some((key, val))
    }
}

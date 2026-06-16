// TASK h1 (algorithmic, hard): Implement an LRU (Least-Recently-Used) cache.
// new LRUCache(capacity): capacity > 0.
//   get(key)      -> value, or -1 if absent. A get COUNTS as a use (most-recent).
//   put(key,val)  -> insert/update; updating counts as a use. If over capacity,
//                    evict the LEAST recently used key.
// All operations should be O(1) average.
class Node {
  constructor(key, value) {
    this.key = key;
    this.value = value;
    this.prev = null;
    this.next = null;
  }
}

class LRUCache {
  constructor(capacity) {
    this.capacity = capacity;
    this.map = new Map();
    this.head = new Node(0, 0);
    this.tail = new Node(0, 0);
    this.head.next = this.tail;
    this.tail.prev = this.head;
  }

  _remove(node) {
    node.prev.next = node.next;
    node.next.prev = node.prev;
  }

  _addToHead(node) {
    node.next = this.head.next;
    node.prev = this.head;
    this.head.next.prev = node;
    this.head.next = node;
  }

  _addToTail(node) {
    node.prev = this.tail.prev;
    node.next = this.tail;
    this.tail.prev.next = node;
    this.tail.prev = node;
  }

  _moveToTail(node) {
    this._remove(node);
    this._addToTail(node);
  }

  get(key) {
    if (!this.map.has(key)) {
      return -1;
    }
    const node = this.map.get(key);
    this._moveToTail(node);
    return node.value;
  }

  put(key, value) {
    if (this.map.has(key)) {
      const node = this.map.get(key);
      node.value = value;
      this._moveToTail(node);
      return;
    }

    let insertAtHead = false;
    if (this.map.size >= this.capacity) {
      const lru = this.head.next;
      this._remove(lru);
      this.map.delete(lru.key);
      insertAtHead = this.map.size > 1;
    }

    const node = new Node(key, value);
    this.map.set(key, node);
    if (insertAtHead) {
      this._addToHead(node);
    } else {
      this._addToTail(node);
    }
  }
}
module.exports = { LRUCache };

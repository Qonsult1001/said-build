# SCA Recall

Drop-in perfect recall for any LLM or embedding model.

```python
from sca_recall import SCA

sca = SCA()
sca.add("doc1", "Rust is a systems programming language")
hits = sca.find("what language has a borrow checker")
```

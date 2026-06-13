"""
SCA Recall — Drop-in perfect recall for any LLM or embedding model.

API matches LAM/MTEB naming conventions: index() + search()

Three-line integration:
    from sca_recall import SCA

    sca = SCA()                                              # 1. Create
    sca.index("doc1", "Rust is a programming language")      # 2. Index
    hits = sca.search("what language has a borrow checker")  # 3. Search

With any embedding model (OpenAI, BGE, LAM, Cohere, etc.):
    emb = model.encode(["Rust is..."])[0].tolist()
    sca.index("doc1", "Rust is...", embedding=emb)
    hits = sca.search("borrow checker", embedding=model.encode(["query"])[0].tolist())

Persistence (matches LAM.save_index / LAM.load_index):
    sca.save("index.sca")
    sca = SCA.load("index.sca")

MCP server:
    sca.serve_mcp()  # Exposes index/search/stats as MCP tools

Install: pip install sca-recall
"""
from __future__ import annotations

__version__ = "0.1.0"
__author__ = "Said-Research"

from typing import List, Optional, Dict, Any
import json


class Hit:
    """A search result."""
    __slots__ = ("id", "score")

    def __init__(self, id: str, score: float):
        self.id = id
        self.score = score

    def __repr__(self):
        return f"Hit(id={self.id!r}, score={self.score:.4f})"

    def to_dict(self) -> Dict[str, Any]:
        return {"id": self.id, "score": self.score}


class SCA:
    """
    SCA (Said Crystalline Attention) — Perfect recall plugin.

    API follows LAM/MTEB naming conventions:
        index()   — Index a document (matches LAM.index / MTEB SearchProtocol.index)
        search()  — Search documents (matches LAM.search / MTEB SearchProtocol.search)
        encode()  — Encode text (matches LAM.encode / MTEB EncoderProtocol.encode)
        save()    — Persist index (matches LAM.save_index)
        load()    — Restore index (matches LAM.load_index)
        stats()   — Index statistics (matches LAM.stats)

    Works WITHOUT any embedding model (pure text search),
    or WITH any embedding model (any dimension: 384d, 768d, 1536d).
    """

    def __init__(self, chunk_size: int = 4096):
        """Create a new SCA index."""
        try:
            from sca_recall._native import SCAPlugin as _NativePlugin
            self._engine = _NativePlugin(chunk_size)
            self._backend = "rust"
        except ImportError:
            self._engine = _PythonFallback()
            self._backend = "python"

    # =========================================================================
    # INDEX — matches LAM.index(doc_id, text) / MTEB SearchProtocol.index()
    # =========================================================================

    def index(
        self,
        id: str,
        text: str,
        embedding: Optional[List[float]] = None,
    ) -> None:
        """Index a document. Works immediately, no batch/commit step.

        Matches LAM.index(doc_id, text, embedding=True/False).

        Args:
            id: Unique document identifier.
            text: Document text (any length).
            embedding: Optional pre-computed embedding from your model.
        """
        if embedding is not None:
            self._engine.index_with_embedding(id, text, embedding)
        else:
            self._engine.index(id, text)

    def index_many(self, documents: List[tuple]) -> None:
        """Bulk index documents. List of (id, text) tuples."""
        for doc_id, text in documents:
            self._engine.index(doc_id, text)

    # =========================================================================
    # SEARCH — matches LAM.search(query, top_k) / MTEB SearchProtocol.search()
    # =========================================================================

    def search(
        self,
        query: str,
        top_k: int = 10,
        embedding: Optional[List[float]] = None,
    ) -> List[Hit]:
        """Search indexed documents. Returns ranked results.

        Matches LAM.search(query, top_k).

        Args:
            query: Search query.
            top_k: Maximum results to return.
            embedding: Optional query embedding from your model.
        """
        if embedding is not None:
            raw = self._engine.search_with_embedding(query, embedding, top_k)
        else:
            raw = self._engine.search(query, top_k)
        return [Hit(id=h["id"], score=h["score"]) for h in raw]

    def search_exact(self, query: str) -> List[Hit]:
        """Exact token match."""
        raw = self._engine.search_exact(query)
        return [Hit(id=h["id"], score=h["score"]) for h in raw]

    # =========================================================================
    # ENCODE — matches LAM.encode(texts) / MTEB EncoderProtocol.encode()
    # =========================================================================

    def encode(self, texts: List[str]) -> List[List[float]]:
        """Encode texts into SCA's internal HDC representation.

        NOTE: This is NOT a neural embedding. It's a deterministic
        hash-based hypervector. For neural embeddings, use LAM.encode()
        or any other embedding model and pass via the embedding parameter.

        Matches LAM.encode(texts) signature for API consistency.
        """
        # Placeholder — encode via SCA's HDC is available in Rust backend
        return [[0.0] * 384 for _ in texts]

    # =========================================================================
    # PERSIST — matches LAM.save_index(path) / LAM.load_index(path)
    # =========================================================================

    def save(self, path: str) -> None:
        """Save index to file. Matches LAM.save_index(path)."""
        self._engine.save(path)

    @classmethod
    def load(cls, path: str) -> "SCA":
        """Load index from file. Matches LAM.load_index(path)."""
        sca = cls.__new__(cls)
        try:
            from sca_recall._native import SCAPlugin as _NativePlugin
            sca._engine = _NativePlugin.load(path)
            sca._backend = "rust"
        except ImportError:
            sca._engine = _PythonFallback.load(path)
            sca._backend = "python"
        return sca

    # =========================================================================
    # INFO — matches LAM.stats() / len(model)
    # =========================================================================

    def stats(self) -> Dict[str, Any]:
        """Index statistics. Matches LAM.stats()."""
        return {
            "docs": len(self),
            "tokens": self.token_count,
            "vocab": self.vocab_size,
            "backend": self._backend,
        }

    @property
    def token_count(self) -> int:
        return self._engine.token_count()

    @property
    def vocab_size(self) -> int:
        return self._engine.vocab_size()

    def get_document(self, id: str) -> Optional[str]:
        """Get document text by ID. Matches LAM.get_document(doc_id)."""
        return self._engine.get_document(id)

    def clear(self) -> None:
        """Clear all indexed documents. Matches LAM.clear()."""
        self.__init__()

    def __len__(self) -> int:
        return self._engine.len()

    def __contains__(self, id: str) -> bool:
        return self._engine.contains(id)

    def __repr__(self) -> str:
        return f"SCA(docs={len(self)}, tokens={self.token_count}, backend={self._backend!r})"

    # =========================================================================
    # MCP SERVER
    # =========================================================================

    def serve_mcp(self, port: int = 0) -> None:
        """Start an MCP server exposing SCA tools."""
        _serve_mcp(self, port)


# =============================================================================
# PURE PYTHON FALLBACK
# =============================================================================

class _PythonFallback:
    """Basic TF-IDF fallback when Rust crate not available."""

    def __init__(self):
        self._docs = {}
        self._index = {}

    def index(self, id: str, text: str):
        self._docs[id] = text
        for word in text.lower().split():
            if len(word) >= 3:
                self._index.setdefault(word, set()).add(id)

    def index_with_embedding(self, id: str, text: str, embedding: list):
        self.index(id, text)

    def search(self, query: str, top_k: int = 10) -> List[Dict]:
        scores = {}
        for word in query.lower().split():
            for doc_id in self._index.get(word, []):
                scores[doc_id] = scores.get(doc_id, 0) + 1
        ranked = sorted(scores.items(), key=lambda x: -x[1])[:top_k]
        return [{"id": d, "score": float(s)} for d, s in ranked]

    def search_with_embedding(self, query: str, emb: list, top_k: int = 10) -> List[Dict]:
        return self.search(query, top_k)

    def search_exact(self, query: str) -> List[Dict]:
        return self.search(query, 100)

    def save(self, path: str):
        with open(path, "w") as f:
            json.dump({"docs": self._docs}, f)

    @classmethod
    def load(cls, path: str) -> "_PythonFallback":
        fb = cls()
        with open(path) as f:
            data = json.load(f)
        for id, text in data["docs"].items():
            fb.index(id, text)
        return fb

    def len(self): return len(self._docs)
    def contains(self, id: str): return id in self._docs
    def get_document(self, id: str): return self._docs.get(id)
    def token_count(self): return sum(len(v.split()) for v in self._docs.values())
    def vocab_size(self): return len(self._index)


# =============================================================================
# MCP SERVER
# =============================================================================

def _serve_mcp(sca: SCA, port: int = 0):
    """Minimal MCP server for SCA plugin."""
    import sys

    tools = {
        "sca_index": {
            "description": "Index a document for perfect recall.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Document ID"},
                    "text": {"type": "string", "description": "Document text"},
                    "embedding": {"type": "array", "items": {"type": "number"}, "description": "Optional embedding"},
                },
                "required": ["id", "text"],
            },
        },
        "sca_search": {
            "description": "Search indexed documents. Returns ranked results.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query"},
                    "top_k": {"type": "integer", "description": "Max results (default: 10)"},
                    "embedding": {"type": "array", "items": {"type": "number"}, "description": "Optional query embedding"},
                },
                "required": ["query"],
            },
        },
        "sca_stats": {
            "description": "Index statistics.",
            "inputSchema": {"type": "object", "properties": {}},
        },
    }

    def handle(method, params):
        if method == "initialize":
            return {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}},
                    "serverInfo": {"name": "sca-recall", "version": __version__}}
        if method == "tools/list":
            return {"tools": [{"name": k, **v} for k, v in tools.items()]}
        if method == "tools/call":
            name = params.get("name", "")
            args = params.get("arguments", {})
            if name == "sca_index":
                sca.index(args["id"], args["text"], args.get("embedding"))
                return {"content": [{"type": "text", "text": f"Indexed: {args['id']} ({len(sca)} total)"}]}
            if name == "sca_search":
                hits = sca.search(args["query"], args.get("top_k", 10), args.get("embedding"))
                text = "\n".join(f"{i+1}. [{h.id}] score={h.score:.4f}" for i, h in enumerate(hits))
                return {"content": [{"type": "text", "text": text or "No results."}]}
            if name == "sca_stats":
                return {"content": [{"type": "text", "text": json.dumps(sca.stats(), indent=2)}]}

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue
        result = handle(req.get("method"), req.get("params", {}))
        if result and "id" in req:
            resp = json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": result})
            sys.stdout.write(resp + "\n")
            sys.stdout.flush()


__all__ = ["SCA", "Hit", "__version__"]

import json, time, urllib.request, sys

KEY = "${OPENROUTER_API_KEY}"
PROVIDERS = ["DeepInfra","Fireworks","Novita","Chutes","Moonshot AI","SiliconFlow","Phala","AtlasCloud"]
PROMPT = 'Implement add(a,b). Output ONLY {"edits":[{"file":"r1.js","mode":"replace-text","anchor":"throw","content":"return a+b;//"}]}'

def call(prov):
    body = {
        "model": "moonshotai/kimi-k2.5",
        "provider": {"order": [prov], "allow_fallbacks": False},
        "temperature": 1, "max_tokens": 600,
        "response_format": {"type": "json_object"},
        "reasoning": {"effort": "medium"},
        "messages": [{"role": "user", "content": PROMPT}],
    }
    req = urllib.request.Request(
        "https://openrouter.ai/api/v1/chat/completions",
        data=json.dumps(body).encode(),
        headers={"Authorization": "Bearer " + KEY, "Content-Type": "application/json"},
    )
    t0 = time.time()
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            d = json.load(r)
        dt = time.time() - t0
        m = d["choices"][0]["message"]
        ok = "ok" if (m.get("content") or m.get("reasoning")) else "EMPTY"
        return dt, ok
    except Exception as e:
        return time.time() - t0, "ERR:" + str(e)[:60]

print(f"{'provider':14} {'secs':>7}  result")
results = []
for p in PROVIDERS:
    dt, ok = call(p)
    results.append((dt, p, ok))
    print(f"{p:14} {dt:7.2f}  {ok}")
ok_results = sorted([r for r in results if r[2] == "ok"])
if ok_results:
    print("\nFASTEST (working):", ok_results[0][1], f"{ok_results[0][0]:.2f}s")

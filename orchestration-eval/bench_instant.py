import json, time, urllib.request

KEY = "${OPENROUTER_API_KEY}"
PROVIDERS = ["Chutes","DeepInfra","Novita","Fireworks","SiliconFlow","Phala","Moonshot AI","AtlasCloud","Together"]
# A REALISTIC coding prompt (not trivial) so timings reflect actual use.
PROMPT = ('Implement add(a,b) in src/r1.js to return the sum. The current file is:\n'
          '```\nfunction add() { throw new Error("not implemented"); }\nmodule.exports={add};\n```\n'
          'Output ONLY JSON: {"edits":[{"file":"src/r1.js","mode":"replace-text","anchor":"throw new Error(\'not implemented\')","content":"return a+b;"}]}')

def call(prov):
    body = {
        "model": "moonshotai/kimi-k2.5",
        "provider": {"order": [prov], "allow_fallbacks": False},
        "temperature": 0.6, "max_tokens": 1024,
        "response_format": {"type": "json_object"},
        "reasoning": {"enabled": False},
        "messages": [{"role": "user", "content": PROMPT}],
    }
    req = urllib.request.Request("https://openrouter.ai/api/v1/chat/completions",
        data=json.dumps(body).encode(),
        headers={"Authorization": "Bearer "+KEY, "Content-Type": "application/json"})
    t0 = time.time()
    try:
        with urllib.request.urlopen(req, timeout=90) as r: d = json.load(r)
        m = d["choices"][0]["message"]
        return time.time()-t0, ("ok" if m.get("content") else "EMPTY")
    except Exception as e:
        return time.time()-t0, "ERR:"+str(e)[:50]

print(f"{'provider':14} {'secs':>6}  result")
res=[]
for p in PROVIDERS:
    dt, ok = call(p); res.append((dt,p,ok)); print(f"{p:14} {dt:6.2f}  {ok}")
okr=sorted([r for r in res if r[2]=="ok"])
print("\nFASTEST instant:", okr[0][1], f"{okr[0][0]:.2f}s" if okr else "none")

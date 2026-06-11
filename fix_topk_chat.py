import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    lines = f.readlines()

for i, line in enumerate(lines):
    if "ReasoningSelect value={reasoning} onChange={setReasoning} />" in line:
        if "topK" not in line and "top-k" not in line and "top_k" not in lines[i+1]:
            lines[i] = line.replace('<ReasoningSelect value={reasoning} onChange={setReasoning} />', '<ReasoningSelect value={reasoning} onChange={setReasoning} />\n          <label style={{ marginLeft: "10px", display: "flex", alignItems: "center", gap: "5px" }}>top-k <input type="number" style={{ width: "60px" }} value={topK} onChange={(e) => { const v = Number(e.target.value); setTopK(v); void api.setSetting("top_k_default", v); }} /></label>')

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.writelines(lines)
print("fixed")

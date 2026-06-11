import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# 1. Remove from SettingsTab
# `const [topK, setTopK] = useState(5);`
text = re.sub(r'  const \[topK, setTopK\] = useState\(5\);\n', '', text)
# `void api.getSetting<number>("top_k_default").then((v) => { if (v != null) setTopK(v); }).catch(() => {});`
text = re.sub(r'    void api\.getSetting<number>\("top_k_default"\)\.then\(\(v\) => \{ if \(v != null\) setTopK\(v\); \}\)\.catch\(\(\) => \{\}\);\n', '', text)
# `      <div className="card">\n        <h3>Retrieval</h3>\n        <label>top-k default\n          <input type="number" value={topK}\n            onChange={(e) => { const v = Number(e.target.value); setTopK(v); void api.setSetting("top_k_default", v); }} />\n        </label>\n      </div>`
text = re.sub(r'      <div className="card">\n\s*<h3>Retrieval</h3>\n\s*<label>top-k default\n\s*<input type="number" value=\{topK\}\n\s*onChange=\{\(e\) => \{ const v = Number\(e.target.value\); setTopK\(v\); void api\.setSetting\("top_k_default", v\); \}\} />\n\s*</label>\n\s*</div>\n', '', text)

# 2. Add to ChatTab
chat_state_add = '  const [topK, setTopK] = useState(5);'
text = text.replace('  const [reasoning, setReasoning] = useState("off");\n', '  const [reasoning, setReasoning] = useState("off");\n' + chat_state_add + '\n')

chat_effect = '      .catch(() => {});\n'
chat_effect_new = '      .catch(() => {});\n    void api.getSetting<number>("top_k_default").then((v) => { if (v != null) setTopK(v); }).catch(() => {});\n'
text = text.replace(chat_effect, chat_effect_new, 1)

text = text.replace('api.chat(props.scope, query, 5, endpointId', 'api.chat(props.scope, query, topK, endpointId')

chat_jsx = '<ReasoningSelect value={reasoning} onChange={setReasoning} />'
chat_jsx_new = '<ReasoningSelect value={reasoning} onChange={setReasoning} />\n          <label style={{ marginLeft: "10px", display: "flex", alignItems: "center", gap: "5px" }}>top-k <input type="number" style={{ width: "60px" }} value={topK} onChange={(e) => { const v = Number(e.target.value); setTopK(v); void api.setSetting("top_k_default", v); }} /></label>'
text = text.replace(chat_jsx, chat_jsx_new, 1)

# 3. Add to GridTab
grid_state_add = '  const [topK, setTopK] = useState(5);'
# In GridTab, reasoning is added at the top:
text = text.replace('  const [reasoning, setReasoning] = useState("off");\n', '  const [reasoning, setReasoning] = useState("off");\n' + grid_state_add + '\n', 1)

grid_effect_new = '      .catch(() => {});\n    void api.getSetting<number>("top_k_default").then((v) => { if (v != null) setTopK(v); }).catch(() => {});\n'
text = text.replace(chat_effect, grid_effect_new, 1)

text = text.replace('api.chat(props.scope, q, 3, endpointId', 'api.chat(props.scope, q, topK, endpointId')

grid_jsx = '<ReasoningSelect value={reasoning} onChange={setReasoning} />'
grid_jsx_new = '<ReasoningSelect value={reasoning} onChange={setReasoning} />\n          <label style={{ marginLeft: "10px", display: "flex", alignItems: "center", gap: "5px" }}>top-k <input type="number" style={{ width: "60px" }} value={topK} onChange={(e) => { const v = Number(e.target.value); setTopK(v); void api.setSetting("top_k_default", v); }} /></label>'
text = text.replace(grid_jsx, grid_jsx_new, 1)


with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)
print("moved top-k")

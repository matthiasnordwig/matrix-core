import re

with open("/Users/matthias.nordwig/matrix/app/src/App.tsx", "r") as f:
    app_text = f.read()

app_text = app_text.replace('{ id: "settings", label: "Settings", icon: "⚙" }', '{ id: "settings", label: "Endpoints", icon: "🔌" }')

with open("/Users/matthias.nordwig/matrix/app/src/App.tsx", "w") as f:
    f.write(app_text)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    pages_text = f.read()

# Remove from SettingsTab
settings_topk_state = '  const [topK, setTopK] = useState(5);\n'
pages_text = pages_text.replace(settings_topk_state, "")

settings_topk_load = '    void api.getSetting<number>("top_k_default").then((v) => { if (v != null) setTopK(v); }).catch(() => {});\n'
pages_text = pages_text.replace(settings_topk_load, "")

settings_topk_ui = """      <div className="card">
        <h3>Retrieval</h3>
        <label>top-k default
          <input type="number" value={topK}
            onChange={(e) => { const v = Number(e.target.value); setTopK(v); void api.setSetting("top_k_default", v); }} />
        </label>
      </div>

"""
pages_text = pages_text.replace(settings_topk_ui, "")
# Also fix the header if I changed Settings to Endpoints in the title
pages_text = pages_text.replace('<h2>Settings</h2>', '<h2>Endpoints</h2>')

# Add to GridTab
grid_state = '  const [reasoning, setReasoning] = useState("off");\n  const [topK, setTopK] = useState(3);\n'
pages_text = pages_text.replace('  const [reasoning, setReasoning] = useState("off");\n', grid_state, 1)

grid_chat_call = 'const ans = await api.chat(props.scope, q, 3, endpointId, reasoning === "off" ? null : reasoning);'
new_grid_chat_call = 'const ans = await api.chat(props.scope, q, topK, endpointId, reasoning === "off" ? null : reasoning);'
pages_text = pages_text.replace(grid_chat_call, new_grid_chat_call)

grid_ui = '<ReasoningSelect value={reasoning} onChange={setReasoning} />'
new_grid_ui = '<ReasoningSelect value={reasoning} onChange={setReasoning} />\n          <label style={{display:"flex", alignItems:"center", gap:"8px"}}>top-k <input type="number" min="1" max="20" value={topK} onChange={(e) => setTopK(Number(e.target.value))} style={{width:"60px", padding:"4px"}} /></label>'
pages_text = pages_text.replace(grid_ui, new_grid_ui)


# Add to ChatTab
chat_state = '  const [reasoning, setReasoning] = useState("off");\n  const [topK, setTopK] = useState(5);\n'
pages_text = pages_text.replace('  const [reasoning, setReasoning] = useState("off");\n', chat_state) # Note: this will replace both GridTab and ChatTab if we're not careful, but GridTab was already replaced since count=1 was used for GridTab. Actually, let me just do a targeted replace for ChatTab.

# Wait, `pages_text.replace('  const [reasoning, setReasoning] = useState("off");\n', chat_state)` will hit ChatTab.
pages_text = pages_text.replace('  const [reasoning, setReasoning] = useState("off");\n', chat_state, 1)

chat_call = 'setAnswer(await api.chat(props.scope, query, 5, endpointId, reasoning === "off" ? null : reasoning));'
new_chat_call = 'setAnswer(await api.chat(props.scope, query, topK, endpointId, reasoning === "off" ? null : reasoning));'
pages_text = pages_text.replace(chat_call, new_chat_call)

chat_ui = '<ReasoningSelect value={reasoning} onChange={setReasoning} />'
new_chat_ui = '<ReasoningSelect value={reasoning} onChange={setReasoning} />\n          <label style={{display:"flex", alignItems:"center", gap:"8px", marginLeft:"16px"}}>top-k <input type="number" min="1" max="20" value={topK} onChange={(e) => setTopK(Number(e.target.value))} style={{width:"60px", padding:"4px"}} /></label>'
pages_text = pages_text.replace(chat_ui, new_chat_ui)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(pages_text)

print("done")

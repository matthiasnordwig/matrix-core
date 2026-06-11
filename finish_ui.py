import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    lines = f.readlines()

# Fix redeclared `editing` (lines 1012 and 1061 or so)
# Delete line 1012 `const [editing, setEditing]`
for i in range(1000, 1100):
    if "const [editing, setEditing] = useState<{kind" in lines[i]:
        if i < 1030:
            lines[i] = ""
            break

# Remove `menuForEndpoint` duplicate if it exists
for i in range(1000, 1100):
    if "const [menuForEndpoint, setMenuForEndpoint]" in lines[i]:
        pass # keep it if it's the first one, actually let's just make sure there's only one.

# Fix the `null` not assignable to `number` error
# This happens in `api.deleteLlmEndpoint(ep.id)` where ep.id might be null? No!
# The error was `src/pages.tsx(1201,69): error TS2345: Argument of type 'number | null' is not assignable to parameter of type 'number'.`
# And 1219. This is in `updateLlmEndpoint` because `editing.id` is `number | null`.
for i, line in enumerate(lines):
    if "if (editing?.kind === \"llm\") await api.updateLlmEndpoint({ id: editing.id, new: payload });" in line:
        lines[i] = '      if (editing?.kind === "llm" && editing.id != null) await api.updateLlmEndpoint({ id: editing.id, new: payload });\n'
    if "if (editing?.kind === \"emb\") await api.updateEmbeddingModel({ id: editing.id, new: payload });" in line:
        lines[i] = '      if (editing?.kind === "emb" && editing.id != null) await api.updateEmbeddingModel({ id: editing.id, new: payload });\n'


# Now extract the endpointForm
# The form starts with `<div className="form">` under `Endpunkt hinzufügen`
form_start = -1
form_end = -1
for i, line in enumerate(lines):
    if '<h3>{editing ? "Endpunkt bearbeiten" : "Endpunkt hinzufügen"}</h3>' in line:
        form_start = i - 1 # <div className="card">
for i in range(form_start + 1, len(lines)):
    if '<table className="grid">' in lines[i]:
        form_end = i - 2 # </div> before table
        break

if form_start != -1 and form_end != -1:
    form_lines = lines[form_start:form_end+1]
    
    # Extract the inner <div className="form">
    inner_start = -1
    for i, line in enumerate(form_lines):
        if '<div className="form">' in line:
            inner_start = i
            break
            
    inner_lines = form_lines[inner_start:]
    
    # endpointForm declaration
    decl = "  const endpointForm = (\n    <>\n" + "".join(inner_lines) + "    </>\n  );\n"
    
    # replace the whole block with {editing == null && card}
    replacement = decl + "      {editing == null && (\n        <div className=\"card\">\n          <h3>Endpunkt hinzufügen</h3>\n          <p className=\"muted\">\n            <b>1.</b> Server-IP → <b>2.</b> Modelle abfragen → <b>3.</b> Modell wählen (Typ wird automatisch erkannt) → <b>4.</b> speichern. Beim Ollama-Server immer die <b> IP des Mac-Rechners</b> eintragen (dort läuft Ollama), nicht die dieses Geräts — dann klappt es auch vom iPhone. Die IP wird gemerkt. <b>Lokal (ONNX)</b> läuft auf dem Gerät selbst (kein Server/IP).\n          </p>\n          {endpointForm}\n        </div>\n      )}\n"
    
    lines = lines[:form_start] + [replacement] + lines[form_end+1:]

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.writelines(lines)

print("done finish_ui")

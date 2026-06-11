import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# 1. Add menuForEndpoint state hook
old_state = "  const [confirmDeleteEmbId, setConfirmDeleteEmbId] = useState<number | null>(null);"
new_state = "  const [confirmDeleteEmbId, setConfirmDeleteEmbId] = useState<number | null>(null);\n  const [menuForEndpoint, setMenuForEndpoint] = useState<string | null>(null);"
text = text.replace(old_state, new_state)

# 2. Find the entire card containing the form:
# <div className="card">
#   <h3>{editing ? "Endpunkt bearbeiten" : "Endpunkt hinzufügen"}</h3>
#   ...
#   </div>
# </div>

pattern_form_card = re.compile(r'      <div className="card">\s*<h3>\{editing \? "Endpunkt bearbeiten" : "Endpunkt hinzufügen"\}</h3>.*?</div>\s*</div>\s*<ul className="list">', re.DOTALL)
match = pattern_form_card.search(text)
if match:
    form_card = match.group(0)
    # Extract just the `<div className="form">...</div>` out of it
    form_div_match = re.search(r'(<div className="form">.*</div>)\s*</div>\s*<ul className="list">', form_card, re.DOTALL)
    if form_div_match:
        form_jsx = form_div_match.group(1)
        # We need to prepend `const endpointForm = (` before the `return (` of SettingsTab
        # and append `);`
        
        # Replace the 'abbrechen' logic to use cancel
        form_jsx = form_jsx.replace('{editing && <button className="link" onClick={resetForm}>abbrechen</button>}', '<button className="link" onClick={resetForm}>Cancel</button>')
        
        text = text.replace("  return (\n    <div className=\"page\">\n      <h2>Settings</h2>", f"  const endpointForm = (\n{form_jsx}\n  );\n\n  return (\n    <div className=\"page\">\n      <h2>Settings</h2>")
        
        # Now remove the old form card entirely from the render
        text = text.replace(form_card, '<ul className="list">')

# 3. Replace the ul list with a table
ul_pattern = re.compile(r'      <ul className="list">\s*\{endpoints\.map.*?</ul>', re.DOTALL)

table_jsx = """      <table className="grid">
        <thead>
          <tr><th>Name / Model</th><th>Type</th><th>Provider</th><th>Config</th><th>Actions</th></tr>
        </thead>
        <tbody>
          {endpoints.map((ep) => (
            <Fragment key={`llm-${ep.id}`}>
              <tr>
                <td>{ep.name}<br/><small className="muted">{ep.model_id}</small></td>
                <td title="Chat Model">💬 Chat</td>
                <td>{ep.provider}</td>
                <td>
                  ctx {ep.context_window} · ∥{ep.max_concurrency}
                  {ep.tpm_limit ? ` · ${ep.tpm_limit} TPM` : ""}{ep.rpm_limit ? ` · ${ep.rpm_limit} RPM` : ""}
                  {ep.api_key_ref ? " · 🔑" : ""}
                </td>
                <td className="col-actions">
                  {confirmDeleteLlmEpId === ep.id ? (
                    <span className="confirm">
                      Delete?
                      <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteLlmEndpoint(ep.id).then(reload)}>✓ Delete</button>
                      <button className="cancel-btn" onClick={() => setConfirmDeleteLlmEpId(null)}>Cancel</button>
                    </span>
                  ) : (
                    <div className="actions-menu">
                      <button className="menu-trigger" title="Actions" onClick={() => setMenuForEndpoint(menuForEndpoint === `llm-${ep.id}` ? null : `llm-${ep.id}`)}>⋮</button>
                      {menuForEndpoint === `llm-${ep.id}` && (
                        <div className="menu-dropdown">
                          <button onClick={() => { setMenuForEndpoint(null); editLlm(ep); }}>✏️ Edit endpoint</button>
                          <button onClick={() => { setMenuForEndpoint(null); setConfirmDeleteLlmEpId(ep.id); }}>🗑️ Delete endpoint</button>
                        </div>
                      )}
                    </div>
                  )}
                </td>
              </tr>
              {editing?.kind === "llm" && editing.id === ep.id && (
                <tr>
                  <td colSpan={5}>
                    {endpointForm}
                  </td>
                </tr>
              )}
            </Fragment>
          ))}
          {models.map((m) => (
            <Fragment key={`emb-${m.id}`}>
              <tr>
                <td>{m.identifier}</td>
                <td title="Embedding Model">🔢 Embedding</td>
                <td>{m.kind}</td>
                <td>
                  {m.default_dim ? `${m.default_dim}d` : "?d"} · ∥{m.max_concurrency}
                  {m.tpm_limit ? ` · ${m.tpm_limit} TPM` : ""}{m.rpm_limit ? ` · ${m.rpm_limit} RPM` : ""}
                </td>
                <td className="col-actions">
                  {confirmDeleteEmbId === m.id ? (
                    <span className="confirm">
                      Delete?
                      <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteEmbeddingModel(m.id).then(reload)}>✓ Delete</button>
                      <button className="cancel-btn" onClick={() => setConfirmDeleteEmbId(null)}>Cancel</button>
                    </span>
                  ) : (
                    <div className="actions-menu">
                      <button className="menu-trigger" title="Actions" onClick={() => setMenuForEndpoint(menuForEndpoint === `emb-${m.id}` ? null : `emb-${m.id}`)}>⋮</button>
                      {menuForEndpoint === `emb-${m.id}` && (
                        <div className="menu-dropdown">
                          <button onClick={() => { setMenuForEndpoint(null); editEmb(m); }}>✏️ Edit model</button>
                          <button onClick={() => { setMenuForEndpoint(null); setConfirmDeleteEmbId(m.id); }}>🗑️ Delete model</button>
                        </div>
                      )}
                    </div>
                  )}
                </td>
              </tr>
              {editing?.kind === "emb" && editing.id === m.id && (
                <tr>
                  <td colSpan={5}>
                    {endpointForm}
                  </td>
                </tr>
              )}
            </Fragment>
          ))}
          {endpoints.length === 0 && models.length === 0 && (
            <tr><td colSpan={5} className="muted" style={{ textAlign: "center" }}>keine Endpunkte</td></tr>
          )}
        </tbody>
      </table>
      
      {editing == null && (
        <div className="card" style={{ marginTop: "2rem" }}>
          <h3>Endpunkt hinzufügen</h3>
          <p className="muted">
            <b>1.</b> Server-IP → <b>2.</b> Modelle abfragen → <b>3.</b> Modell wählen (Typ wird
            automatisch erkannt) → <b>4.</b> speichern. Beim Ollama-Server immer die
            <b> IP des Mac-Rechners</b> eintragen (dort läuft Ollama), nicht die dieses Geräts —
            dann klappt es auch vom iPhone. Die IP wird gemerkt. <b>Lokal (ONNX)</b> läuft auf
            dem Gerät selbst (kein Server/IP).
          </p>
          {endpointForm}
        </div>
      )}"""

text = ul_pattern.sub(table_jsx, text, count=1)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("Settings patched.")

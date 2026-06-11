import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# 1. Update ProfilesTab LLM lists
llm_list_old = """        <ul className="list">
          {profiles.map((p) => (
            <li key={p.id}>
              <b>{p.name}</b> · {p.overlap_ratio * 100}% overlap, endpoint {p.llm_endpoint_id}
              <button className="link" onClick={() => startEditLlm(p)}>edit</button>
              {confirmDeleteLlmId === p.id ? (
                <span className="confirm" style={{ marginLeft: "10px" }}>
                  <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteChunkingProfile({ id: p.id }).then(() => { setConfirmDeleteLlmId(null); reload(); })}>✓ Delete</button>
                  <button className="cancel-btn" onClick={() => setConfirmDeleteLlmId(null)}>Cancel</button>
                </span>
              ) : (
                <button className="link" onClick={() => setConfirmDeleteLlmId(p.id)}>delete</button>
              )}
            </li>
          ))}
          {profiles.length === 0 && <li className="muted">keine Profil-Vorlagen</li>}
        </ul>"""

llm_list_new = """        <table className="grid">
          <thead>
            <tr><th>Name</th><th>Configuration</th><th>Actions</th></tr>
          </thead>
          <tbody>
            {profiles.map((p) => (
              <Fragment key={p.id}>
                <tr>
                  <td>{p.name}</td>
                  <td>{p.overlap_ratio * 100}% overlap, endpoint {p.llm_endpoint_id}</td>
                  <td className="col-actions">
                    {confirmDeleteLlmId === p.id ? (
                      <span className="confirm">
                        Delete?
                        <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteChunkingProfile({ id: p.id }).then(() => { setConfirmDeleteLlmId(null); reload(); })}>✓</button>
                        <button className="cancel-btn" onClick={() => setConfirmDeleteLlmId(null)}>Cancel</button>
                      </span>
                    ) : (
                      <div className="actions-menu">
                        <button className="menu-trigger" title="Actions" onClick={() => setMenuForLlm(menuForLlm === p.id ? null : p.id)}>⋯</button>
                        {menuForLlm === p.id && (
                          <>
                            <div className="menu-backdrop" onClick={() => setMenuForLlm(null)} />
                            <div className="row-menu">
                              <button onClick={() => { setMenuForLlm(null); startEditLlm(p); }}>✏️ Edit profile</button>
                              <button onClick={() => { setMenuForLlm(null); setConfirmDeleteLlmId(p.id); }}>🗑️ Delete profile</button>
                            </div>
                          </>
                        )}
                      </div>
                    )}
                  </td>
                </tr>
                {editingId === p.id && (
                  <tr>
                    <td colSpan={3}>
                      {llmForm}
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
            {profiles.length === 0 && <tr><td colSpan={3} className="muted" style={{textAlign:"center"}}>keine Profil-Vorlagen</td></tr>}
          </tbody>
        </table>"""

text = text.replace(llm_list_old, llm_list_new)

# Update structural profiles list
str_list_old = """        <ul className="list">
          {structuralProfiles.map((p) => (
            <li key={p.id}>
              <b>{p.name}</b> · target {p.min_chunk_chars}-{p.max_chunk_chars}c · {p.patterns?.length ?? 0} Muster
              <button className="link" onClick={() => startEditStr(p)}>edit</button>
              {confirmDeleteStrId === p.id ? (
                <span className="confirm" style={{ marginLeft: "10px" }}>
                  <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteStructuralProfile({ id: p.id }).then(() => { setConfirmDeleteStrId(null); reload(); })}>✓ Delete</button>
                  <button className="cancel-btn" onClick={() => setConfirmDeleteStrId(null)}>Cancel</button>
                </span>
              ) : (
                <button className="link" onClick={() => setConfirmDeleteStrId(p.id)}>delete</button>
              )}
            </li>
          ))}
          {structuralProfiles.length === 0 && <li className="muted">keine Profil-Vorlagen</li>}
        </ul>"""

str_list_new = """        <table className="grid">
          <thead>
            <tr><th>Name</th><th>Configuration</th><th>Actions</th></tr>
          </thead>
          <tbody>
            {structuralProfiles.map((p) => (
              <Fragment key={p.id}>
                <tr>
                  <td>{p.name}</td>
                  <td>target {p.min_chunk_chars}-{p.max_chunk_chars}c · {p.patterns?.length ?? 0} Muster</td>
                  <td className="col-actions">
                    {confirmDeleteStrId === p.id ? (
                      <span className="confirm">
                        Delete?
                        <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteStructuralProfile({ id: p.id }).then(() => { setConfirmDeleteStrId(null); reload(); })}>✓</button>
                        <button className="cancel-btn" onClick={() => setConfirmDeleteStrId(null)}>Cancel</button>
                      </span>
                    ) : (
                      <div className="actions-menu">
                        <button className="menu-trigger" title="Actions" onClick={() => setMenuForStr(menuForStr === p.id ? null : p.id)}>⋯</button>
                        {menuForStr === p.id && (
                          <>
                            <div className="menu-backdrop" onClick={() => setMenuForStr(null)} />
                            <div className="row-menu">
                              <button onClick={() => { setMenuForStr(null); startEditStr(p); }}>✏️ Edit profile</button>
                              <button onClick={() => { setMenuForStr(null); setConfirmDeleteStrId(p.id); }}>🗑️ Delete profile</button>
                            </div>
                          </>
                        )}
                      </div>
                    )}
                  </td>
                </tr>
                {editingStrId === p.id && (
                  <tr>
                    <td colSpan={3}>
                      {strForm}
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
            {structuralProfiles.length === 0 && <tr><td colSpan={3} className="muted" style={{textAlign:"center"}}>keine Profil-Vorlagen</td></tr>}
          </tbody>
        </table>"""

text = text.replace(str_list_old, str_list_new)

# Add menu state for profiles IF not exists
menu_state = """  const [menuForLlm, setMenuForLlm] = useState<number | null>(null);
  const [menuForStr, setMenuForStr] = useState<number | null>(null);"""
if "menuForLlm" not in text:
    text = text.replace('  const [confirmDeleteStrId, setConfirmDeleteStrId] = useState<number | null>(null);', '  const [confirmDeleteStrId, setConfirmDeleteStrId] = useState<number | null>(null);\n' + menu_state)


# 2. Update SettingsTab
old_settings_state = """export function SettingsTab() {
  const [models, setModels] = useState<EmbeddingModel[]>([]);
  const [endpoints, setEndpoints] = useState<LlmEndpoint[]>([]);"""

new_settings_state = """export function SettingsTab() {
  const [models, setModels] = useState<EmbeddingModel[]>([]);
  const [endpoints, setEndpoints] = useState<LlmEndpoint[]>([]);
  const [editing, setEditing] = useState<{kind: "llm" | "emb", id: number | null} | null>(null);
  const [menuForEndpoint, setMenuForEndpoint] = useState<string | null>(null);"""
text = text.replace(old_settings_state, new_settings_state)

old_edit_fns = """  const editLlm = (ep: LlmEndpoint) => {
    setLlmId(ep.id);
    setName(ep.name);
    setBaseUrl(ep.base_url);
    setModelId(ep.model_id);
    setProvider(ep.provider);
    setKeyRef(ep.api_key_ref ?? "");
    setWindowTokens(ep.window_tokens);
    setContextWindow(ep.context_window);
    setConcurrency(ep.max_concurrency);
    setTpm(ep.tpm_limit);
    setRpm(ep.rpm_limit);
  };

  const editEmb = (m: EmbeddingModel) => {
    setEmbId(m.id);
    setEmbIdent(m.identifier);
    setEmbKind(m.kind);
    setEmbProvider(m.api_config ? (JSON.parse(m.api_config).base_url ?? "") : "");
    setEmbModelId(m.api_config ? (JSON.parse(m.api_config).model ?? "") : "");
    setEmbConcurrency(m.max_concurrency);
    setEmbTpm(m.tpm_limit);
    setEmbRpm(m.rpm_limit);
  };"""

new_edit_fns = """  const editLlm = (ep: LlmEndpoint) => {
    setEditing({ kind: "llm", id: ep.id });
    setLlmId(ep.id);
    setName(ep.name);
    setBaseUrl(ep.base_url);
    setModelId(ep.model_id);
    setProvider(ep.provider);
    setKeyRef(ep.api_key_ref ?? "");
    setWindowTokens(ep.window_tokens);
    setContextWindow(ep.context_window);
    setConcurrency(ep.max_concurrency);
    setTpm(ep.tpm_limit);
    setRpm(ep.rpm_limit);
  };

  const editEmb = (m: EmbeddingModel) => {
    setEditing({ kind: "emb", id: m.id });
    setEmbId(m.id);
    setEmbIdent(m.identifier);
    setEmbKind(m.kind);
    setEmbProvider(m.api_config ? (JSON.parse(m.api_config).base_url ?? "") : "");
    setEmbModelId(m.api_config ? (JSON.parse(m.api_config).model ?? "") : "");
    setEmbConcurrency(m.max_concurrency);
    setEmbTpm(m.tpm_limit);
    setEmbRpm(m.rpm_limit);
  };"""
text = text.replace(old_edit_fns, new_edit_fns)

old_reset_fn = """  const resetForm = () => {
    setLlmId(null);
    setEmbId(null);"""

new_reset_fn = """  const resetForm = () => {
    setEditing(null);
    setLlmId(null);
    setEmbId(null);"""
text = text.replace(old_reset_fn, new_reset_fn)

endpoints_list_old = """        <ul className="list">
          {endpoints.map((ep) => (
            <li key={`llm-${ep.id}`}>
              <b>{ep.name}</b> · 💬 chat · {ep.provider} · {ep.model_id} · ctx {ep.context_window} · ∥{ep.max_concurrency}
              {ep.tpm_limit ? ` · ${ep.tpm_limit} TPM` : ""}{ep.rpm_limit ? ` · ${ep.rpm_limit} RPM` : ""}
              {ep.api_key_ref ? " · 🔑" : ""}
              <button className="link" onClick={() => editLlm(ep)}>edit</button>
              {confirmDeleteLlmEpId === ep.id ? (
                <span className="confirm" style={{ marginLeft: "10px" }}>
                  <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteLlmEndpoint(ep.id).then(reload)}>✓ Delete</button>
                  <button className="cancel-btn" onClick={() => setConfirmDeleteLlmEpId(null)}>Cancel</button>
                </span>
              ) : (
                <button className="link" onClick={() => setConfirmDeleteLlmEpId(ep.id)}>delete</button>
              )}
            </li>
          ))}
          {models.map((m) => (
            <li key={`emb-${m.id}`}>
              <b>{m.identifier}</b> · 🔢 embedding · {m.kind} · {m.default_dim ? `${m.default_dim}d` : "?d"} · ∥{m.max_concurrency}
              {m.tpm_limit ? ` · ${m.tpm_limit} TPM` : ""}{m.rpm_limit ? ` · ${m.rpm_limit} RPM` : ""}
              <button className="link" onClick={() => editEmb(m)}>edit</button>
              {confirmDeleteEmbId === m.id ? (
                <span className="confirm" style={{ marginLeft: "10px" }}>
                  <button className="icon-btn danger" title="Confirm delete" onClick={() => void api.deleteEmbeddingModel(m.id).then(reload)}>✓ Delete</button>
                  <button className="cancel-btn" onClick={() => setConfirmDeleteEmbId(null)}>Cancel</button>
                </span>
              ) : (
                <button className="link" onClick={() => setConfirmDeleteEmbId(m.id)}>delete</button>
              )}
            </li>
          ))}
          {endpoints.length === 0 && models.length === 0 && <li className="muted">keine Endpunkte</li>}
        </ul>"""

endpoints_list_new = """        <table className="grid">
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
                        <button className="menu-trigger" title="Actions" onClick={() => setMenuForEndpoint(menuForEndpoint === `llm-${ep.id}` ? null : `llm-${ep.id}`)}>⋯</button>
                        {menuForEndpoint === `llm-${ep.id}` && (
                          <>
                            <div className="menu-backdrop" onClick={() => setMenuForEndpoint(null)} />
                            <div className="row-menu">
                              <button onClick={() => { setMenuForEndpoint(null); editLlm(ep); }}>✏️ Edit endpoint</button>
                              <button onClick={() => { setMenuForEndpoint(null); setConfirmDeleteLlmEpId(ep.id); }}>🗑️ Delete endpoint</button>
                            </div>
                          </>
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
                        <button className="menu-trigger" title="Actions" onClick={() => setMenuForEndpoint(menuForEndpoint === `emb-${m.id}` ? null : `emb-${m.id}`)}>⋯</button>
                        {menuForEndpoint === `emb-${m.id}` && (
                          <>
                            <div className="menu-backdrop" onClick={() => setMenuForEndpoint(null)} />
                            <div className="row-menu">
                              <button onClick={() => { setMenuForEndpoint(null); editEmb(m); }}>✏️ Edit model</button>
                              <button onClick={() => { setMenuForEndpoint(null); setConfirmDeleteEmbId(m.id); }}>🗑️ Delete model</button>
                            </div>
                          </>
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
        </table>"""

text = text.replace(endpoints_list_old, endpoints_list_new)

# Add missing chunk_endpoint_id in save
old_save = """      chunking_strategy: chunkingStrategy,
      chunking_profile_id: chunkingStrategy === "prompt" ? profileId : null,
      structural_profile_id: chunkingStrategy === "structural" ? structuralProfileId : null,
      embedding_model_id: modelId,
      embedding_dim: model ? model.default_dim : null,
    };"""
new_save = """      chunking_strategy: chunkingStrategy,
      chunking_profile_id: chunkingStrategy === "prompt" ? profileId : null,
      structural_profile_id: chunkingStrategy === "structural" ? structuralProfileId : null,
      embedding_model_id: modelId,
      embedding_dim: model ? model.default_dim : null,
      chunk_endpoint_id: chunkingStrategy === "prompt" ? chunkEndpointId : null,
    };"""
text = text.replace(old_save, new_save)


# Now we must extract the form and wrap it in endpointForm
match = re.search(r'(<div className="form">.*?</>\n\s*\)\}\n\s*</div>)', text, re.DOTALL)
if match:
    form_content = match.group(1)
    endpoint_form_decl = "  const endpointForm = (\n    <>\n" + "\n".join(["      " + l for l in form_content.split("\n")]) + "\n    </>\n  );\n"
    text = text.replace('  return (\n    <div className="tab-content">', endpoint_form_decl + '\n  return (\n    <div className="tab-content">')
    old_card = r'<div className="card">\n\s*<h3>\{(llmId != null \|\| embId != null) \? "Endpunkt bearbeiten" : "Endpunkt hinzufügen"\}</h3>\n\s*<p className="muted">.*?</p>\n\s*<div className="form">.*?</>\n\s*\)\}\n\s*</div>\n\s*</div>'
    text = re.sub(old_card, '{editing == null && (<div className="card"><h3>Endpunkt hinzufügen</h3><p className="muted"><b>1.</b> Server-IP → <b>2.</b> Modelle abfragen → <b>3.</b> Modell wählen (Typ wird automatisch erkannt) → <b>4.</b> speichern. Beim Ollama-Server immer die <b> IP des Mac-Rechners</b> eintragen (dort läuft Ollama), nicht die dieses Geräts — dann klappt es auch vom iPhone. Die IP wird gemerkt. <b>Lokal (ONNX)</b> läuft auf dem Gerät selbst (kein Server/IP).</p>{endpointForm}</div>)}', text, flags=re.DOTALL)


with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("UI successfully rebuilt correctly!")

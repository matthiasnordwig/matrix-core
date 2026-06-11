import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Fix save() missing chunk_endpoint_id
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


# We need to add the endpoint inline editing state and `endpointForm` variable in EndpointsTab
old_endpoints_state = """export function SettingsTab() {
  const [endpoints, setEndpoints] = useState<LlmEndpoint[]>([]);
  const [models, setModels] = useState<EmbeddingModel[]>([]);"""

new_endpoints_state = """export function SettingsTab() {
  const [endpoints, setEndpoints] = useState<LlmEndpoint[]>([]);
  const [models, setModels] = useState<EmbeddingModel[]>([]);
  const [editing, setEditing] = useState<{kind: "llm" | "emb", id: number | null} | null>(null);
  const [menuForEndpoint, setMenuForEndpoint] = useState<string | null>(null);"""
text = text.replace(old_endpoints_state, new_endpoints_state)


# Add the start edit helpers
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


# Also add `endpointForm`
# Wait, I already added `{editing?.kind === "llm" && editing.id === ep.id && (<tr>...{endpointForm}...</tr>)}`
# But `endpointForm` is undefined! Because the previous code didn't define it. It just rendered it inline under `card`.
old_endpoint_card = """      <div className="card">
        <h3>{editing ? "Endpunkt bearbeiten" : "Endpunkt hinzufügen"}</h3>
        <p className="muted">
          <b>1.</b> Server-IP → <b>2.</b> Modelle abfragen → <b>3.</b> Modell wählen (Typ wird
          automatisch erkannt) → <b>4.</b> speichern. Beim Ollama-Server immer die
          <b> IP des Mac-Rechners</b> eintragen (dort läuft Ollama), nicht die dieses Geräts —
          dann klappt es auch vom iPhone. Die IP wird gemerkt. <b>Lokal (ONNX)</b> läuft auf
          dem Gerät selbst (kein Server/IP).
        </p>
        <div className="form">"""

# wait, `editing` is used here: `<h3>{editing ? "Endpunkt bearbeiten" : "Endpunkt hinzufügen"}</h3>`
# Oh! The original code used `llmId != null || embId != null` to determine if editing!
# Let me look at the code!
with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("done")

import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Fix state in SettingsTab
text = text.replace('  const [endpoints, setEndpoints] = useState<LlmEndpoint[]>([]);', '  const [endpoints, setEndpoints] = useState<LlmEndpoint[]>([]);\n  const [editing, setEditing] = useState<{kind: "llm" | "emb", id: number | null} | null>(null);\n  const [menuForEndpoint, setMenuForEndpoint] = useState<string | null>(null);')

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("fixed state")

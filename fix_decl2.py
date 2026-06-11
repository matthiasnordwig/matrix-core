import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    lines = f.readlines()

for i in range(540, 580):
    if "const [menuForLlm, setMenuForLlm]" in lines[i]:
        lines[i] = ""
    if "const [menuForStr, setMenuForStr]" in lines[i]:
        lines[i] = ""

# Now re-add them once exactly where confirmDeleteStrId is
for i in range(540, 580):
    if "const [confirmDeleteStrId, setConfirmDeleteStrId]" in lines[i]:
        lines[i] = '  const [confirmDeleteStrId, setConfirmDeleteStrId] = useState<number | null>(null);\n  const [menuForLlm, setMenuForLlm] = useState<number | null>(null);\n  const [menuForStr, setMenuForStr] = useState<number | null>(null);\n'
        break

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.writelines(lines)
print("fixed")

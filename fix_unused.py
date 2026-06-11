import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    lines = f.readlines()

for i, line in enumerate(lines):
    if "export function ContextsTab" in line:
        for j in range(i, i+50):
            if "const [menuForLlm, setMenuForLlm]" in lines[j]:
                lines[j] = ""
            if "const [menuForStr, setMenuForStr]" in lines[j]:
                lines[j] = ""
        break

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.writelines(lines)
print("removed unused")

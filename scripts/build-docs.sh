#!/usr/bin/env bash
# scripts/build-docs.sh — Convert docs/*.md to styled HTML pages.
# Runs during CI to ensure doc pages always match the source markdown.
#
# Usage: bash scripts/build-docs.sh [output_dir]
# Default output: _site_docs/

set -euo pipefail

DOCS_DIR="docs"
OUT_DIR="${1:-_site_docs}"

mkdir -p "$OUT_DIR"

# Map of filename (without .md) to page title
declare -A TITLES=(
    [getting-started]="Getting Started"
    [oauth]="OAuth Authentication"
    [jetstream]="Jetstream Event Streaming"
    [xrpc]="XRPC Procedures"
    [codegen]="Lexicon Code Generation"
    [testing]="Testing"
    [migration]="Migration Guide"
)

# Ordered sidebar entries
SIDEBAR_ENTRIES=(
    "getting-started:Getting Started"
    "oauth:OAuth"
    "jetstream:Jetstream"
    "xrpc:XRPC"
    "codegen:Code Generation"
    "testing:Testing"
    "migration:Migration Guide"
)

for md_file in "$DOCS_DIR"/*.md; do
    basename=$(basename "$md_file" .md)
    title="${TITLES[$basename]:-$basename}"
    out_file="$OUT_DIR/${basename}.html"

    echo "  📄 $md_file → $out_file"

    # Build sidebar HTML with active state
    sidebar_html=""
    for entry in "${SIDEBAR_ENTRIES[@]}"; do
        slug="${entry%%:*}"
        label="${entry#*:}"
        if [ "$slug" = "$basename" ]; then
            sidebar_html="$sidebar_html<a href=\"/at-rust-go/docs/${slug}.html\" class=\"active\">${label}</a>"
        else
            sidebar_html="$sidebar_html<a href=\"/at-rust-go/docs/${slug}.html\">${label}</a>"
        fi
    done

    # Convert markdown to HTML using Python
    python3 - "$md_file" "$title" "$sidebar_html" "$out_file" << 'PYEOF'
import sys, re, html

md_path = sys.argv[1]
title = sys.argv[2]
sidebar = sys.argv[3]
out_path = sys.argv[4]

with open(md_path, 'r') as f:
    md = f.read()

# --- Markdown to HTML conversion (no dependencies) ---

lines = md.split('\n')
html_lines = []
in_code_block = False
code_lang = ''
code_buf = []
in_list = False
in_table = False
table_buf = []

def flush_list():
    global in_list
    if in_list:
        html_lines.append('</ul>')
        in_list = False

def flush_table():
    global in_table, table_buf
    if in_table:
        # table_buf: list of rows, each is list of cells
        # First row is header, second is separator (skip), rest are data
        html_lines.append('<table>')
        if len(table_buf) > 0:
            html_lines.append('<thead><tr>')
            for cell in table_buf[0]:
                html_lines.append(f'<th>{inline(cell.strip())}</th>')
            html_lines.append('</tr></thead>')
        html_lines.append('<tbody>')
        for row in table_buf[2:]:  # skip header + separator
            html_lines.append('<tr>')
            for cell in row:
                html_lines.append(f'<td>{inline(cell.strip())}</td>')
            html_lines.append('</tr>')
        html_lines.append('</tbody></table>')
        in_table = False
        table_buf = []

def inline(text):
    """Convert inline markdown: bold, italic, code, links."""
    # Code spans first (prevent further processing inside them)
    parts = []
    last = 0
    for m in re.finditer(r'`([^`]+)`', text):
        parts.append(process_inline(text[last:m.start()]))
        parts.append(f'<code>{html.escape(m.group(1))}</code>')
        last = m.end()
    parts.append(process_inline(text[last:]))
    return ''.join(parts)

def process_inline(text):
    """Process bold, italic, links (but not code spans)."""
    text = re.sub(r'\*\*(.+?)\*\*', r'<strong>\1</strong>', text)
    text = re.sub(r'\*(.+?)\*', r'<em>\1</em>', text)
    text = re.sub(r'\[([^\]]+)\]\(([^)]+)\)', r'<a href="\2">\1</a>', text)
    return text

for line in lines:
    # Code blocks
    if line.startswith('```'):
        if in_code_block:
            escaped = html.escape('\n'.join(code_buf))
            html_lines.append(f'<pre><code>{escaped}</code></pre>')
            code_buf = []
            in_code_block = False
        else:
            flush_list()
            flush_table()
            in_code_block = True
            code_lang = line[3:].strip()
        continue

    if in_code_block:
        code_buf.append(line)
        continue

    # Table rows
    if '|' in line and line.strip().startswith('|'):
        flush_list()
        cells = [c for c in line.split('|')[1:-1]]  # strip outer empty strings
        if all(c.strip().replace('-', '') == '' for c in cells):
            # This is the separator row
            if not in_table:
                in_table = True
            table_buf.append(cells)
        else:
            if not in_table:
                in_table = True
            table_buf.append(cells)
        continue
    else:
        flush_table()

    stripped = line.strip()

    # Blank line
    if not stripped:
        flush_list()
        continue

    # Headings
    if stripped.startswith('### '):
        flush_list()
        html_lines.append(f'<h3>{inline(stripped[4:])}</h3>')
        continue
    if stripped.startswith('## '):
        flush_list()
        html_lines.append(f'<h2>{inline(stripped[3:])}</h2>')
        continue
    if stripped.startswith('# '):
        flush_list()
        html_lines.append(f'<h1>{inline(stripped[2:])}</h1>')
        continue

    # Unordered list items
    if stripped.startswith('- ') or stripped.startswith('* '):
        if not in_list:
            html_lines.append('<ul>')
            in_list = True
        html_lines.append(f'<li>{inline(stripped[2:])}</li>')
        continue

    # Numbered list items
    m = re.match(r'^(\d+)\.\s+(.+)', stripped)
    if m:
        if not in_list:
            html_lines.append('<ol>')
            in_list = True
        html_lines.append(f'<li>{inline(m.group(2))}</li>')
        continue

    # Paragraph
    flush_list()
    html_lines.append(f'<p>{inline(stripped)}</p>')

flush_list()
flush_table()

content = '\n'.join(html_lines)

# --- Write HTML with template ---
template = f'''<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{html.escape(title)} — atrg docs</title>
  <style>
    :root {{
      --bg: #0d1117; --card-bg: #161b22; --text: #e6edf3;
      --muted: #8b949e; --accent: #58a6ff; --code-bg: #1c2128; --border: #30363d;
    }}
    * {{ margin: 0; padding: 0; box-sizing: border-box; }}
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: var(--bg); color: var(--text); line-height: 1.7; }}
    nav {{ background: var(--card-bg); border-bottom: 1px solid var(--border); padding: 0.75rem 1.5rem; display: flex; align-items: center; gap: 2rem; position: sticky; top: 0; z-index: 10; }}
    nav a {{ color: var(--muted); text-decoration: none; font-size: 0.9rem; }}
    nav a:hover {{ color: var(--accent); }}
    nav .brand {{ color: var(--text); font-weight: 700; font-size: 1.1rem; }}
    .layout {{ display: flex; max-width: 1100px; margin: 0 auto; min-height: calc(100vh - 52px); }}
    .sidebar {{ width: 220px; padding: 1.5rem 1rem; border-right: 1px solid var(--border); position: sticky; top: 52px; height: fit-content; flex-shrink: 0; }}
    .sidebar a {{ display: block; padding: 0.35rem 0.5rem; color: var(--muted); text-decoration: none; font-size: 0.85rem; border-radius: 4px; }}
    .sidebar a:hover, .sidebar a.active {{ color: var(--accent); background: rgba(88,166,255,0.08); }}
    .sidebar h4 {{ color: var(--text); font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.05em; margin: 1rem 0 0.5rem 0.5rem; }}
    .content {{ flex: 1; padding: 2rem 3rem; max-width: 800px; }}
    .content h1 {{ font-size: 1.8rem; margin-bottom: 1rem; }}
    .content h2 {{ font-size: 1.3rem; margin-top: 2rem; margin-bottom: 0.5rem; color: var(--accent); }}
    .content h3 {{ font-size: 1.1rem; margin-top: 1.5rem; margin-bottom: 0.5rem; }}
    .content p {{ margin-bottom: 1rem; }}
    .content ul, .content ol {{ margin: 0.5rem 0 1rem 1.5rem; }}
    .content li {{ margin-bottom: 0.3rem; }}
    .content code {{ background: var(--code-bg); padding: 2px 6px; border-radius: 3px; font-size: 0.85em; color: #f0883e; font-family: "SF Mono", "Fira Code", monospace; }}
    .content pre {{ background: var(--code-bg); padding: 1rem; border-radius: 6px; overflow-x: auto; margin: 1rem 0; border: 1px solid var(--border); }}
    .content pre code {{ background: none; padding: 0; color: var(--text); }}
    .content a {{ color: var(--accent); }}
    .content table {{ width: 100%; border-collapse: collapse; margin: 1rem 0; }}
    .content th, .content td {{ padding: 0.5rem 0.75rem; border: 1px solid var(--border); text-align: left; font-size: 0.9rem; }}
    .content th {{ background: var(--card-bg); }}
    .content strong {{ color: #f0f6fc; }}
    @media (max-width: 768px) {{ .sidebar {{ display: none; }} .content {{ padding: 1.5rem; }} }}
  </style>
</head>
<body>
  <nav>
    <a href="/at-rust-go/" class="brand">atrg</a>
    <a href="/at-rust-go/docs/getting-started.html">Docs</a>
    <a href="/at-rust-go/api/atrg_core/">API Reference</a>
    <a href="https://github.com/tellmeY18/at-rust-go">GitHub</a>
  </nav>
  <div class="layout">
    <aside class="sidebar">
      <h4>Guide</h4>
      {sidebar}
      <h4>Reference</h4>
      <a href="/at-rust-go/api/atrg_core/">API Docs</a>
      <a href="/at-rust-go/llms.txt">llms.txt</a>
    </aside>
    <main class="content">
{content}
    </main>
  </div>
</body>
</html>'''

with open(out_path, 'w') as f:
    f.write(template)
PYEOF

done

echo ""
echo "✅ Generated $(ls "$OUT_DIR"/*.html 2>/dev/null | wc -l | tr -d ' ') doc pages in $OUT_DIR/"

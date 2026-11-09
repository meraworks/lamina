#!/usr/bin/env python3
"""Check the GitHub issue forms in .github/ISSUE_TEMPLATE.

GitHub shows a broken form as a plain issue, or not at all, without an error.
This script checks the rules of the GitHub issue-form syntax that we use:

- each form parses and has `name`, `description` and `body`,
- each body item has a known `type` and the attributes that the type needs,
- `id`s are unique and use only letters, digits, `-` and `_`,
- dropdown and checkbox options are not empty and unique,
- `config.yml` has a boolean `blank_issues_enabled` and contact links with a
  `name`, an https `url` and `about`.

It needs only Python 3. It reads YAML with PyYAML if you have it, and else with
a small reader for the plain YAML that the forms use.

    python3 scripts/check_issue_forms.py [folder]

The tests are in scripts/test_check_issue_forms.py.
"""
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FORMS = os.path.join(ROOT, ".github", "ISSUE_TEMPLATE")

TYPES = {"markdown", "textarea", "input", "dropdown", "checkboxes"}
ID = re.compile(r"^[A-Za-z0-9_-]+$")
TOP_KEYS = {"name", "description", "body", "title", "labels", "assignees", "projects", "type"}
ATTRS = {
    "markdown": {"value"},
    "textarea": {"label", "description", "placeholder", "value", "render"},
    "input": {"label", "description", "placeholder", "value"},
    "dropdown": {"label", "description", "options", "multiple", "default"},
    "checkboxes": {"label", "description", "options"},
}


# ---------------------------------------------------------------- YAML ---


def load_yaml(text):
    """Parse YAML with PyYAML if it is there, else with `mini_yaml`."""
    try:
        import yaml
    except ImportError:
        return mini_yaml(text)
    return yaml.safe_load(text)


class YamlError(ValueError):
    pass


def mini_yaml(text):
    """Read the plain YAML of issue forms: block mappings and lists, `|` and `>`
    blocks, flow lists, quoted and plain scalars and comments. It raises
    YamlError for anything else, so a form it cannot read fails the check."""
    lines = []
    for n, raw in enumerate(text.splitlines(), 1):
        if "\t" in raw[: len(raw) - len(raw.lstrip())]:
            raise YamlError(f"line {n}: a tab in the indent")
        lines.append((n, raw))
    pos = [0]

    def skip_blank():
        while pos[0] < len(lines):
            s = lines[pos[0]][1].strip()
            if s and not s.startswith("#"):
                return
            pos[0] += 1

    def indent(raw):
        return len(raw) - len(raw.lstrip(" "))

    def block(min_indent):
        skip_blank()
        if pos[0] >= len(lines):
            return None
        n, raw = lines[pos[0]]
        ind = indent(raw)
        if ind < min_indent:
            return None
        if raw.strip().startswith("- ") or raw.strip() == "-":
            return seq(ind)
        return mapping(ind)

    def seq(ind):
        out = []
        while True:
            skip_blank()
            if pos[0] >= len(lines):
                return out
            n, raw = lines[pos[0]]
            if indent(raw) < ind:
                return out
            if indent(raw) > ind:
                raise YamlError(f"line {n}: bad indent")
            s = raw.strip()
            if not (s.startswith("- ") or s == "-"):
                return out
            rest = s[1:].lstrip()
            if not rest:
                pos[0] += 1
                out.append(block(ind + 1))
                continue
            # "- key: value" starts a mapping at the column of `key`.
            col = ind + (len(s) - len(rest))
            if key_value(rest) is not None:
                lines[pos[0]] = (n, " " * col + rest)
                out.append(mapping(col))
            else:
                pos[0] += 1
                out.append(scalar(rest, n))

    def mapping(ind):
        out = {}
        while True:
            skip_blank()
            if pos[0] >= len(lines):
                return out
            n, raw = lines[pos[0]]
            if indent(raw) < ind:
                return out
            if indent(raw) > ind:
                raise YamlError(f"line {n}: bad indent")
            kv = key_value(raw.strip())
            if kv is None:
                if raw.strip().startswith("-"):
                    return out
                raise YamlError(f"line {n}: expected `key: value`")
            k, v = kv
            if k in out:
                raise YamlError(f"line {n}: the key `{k}` is there two times")
            pos[0] += 1
            if v in ("|", "|-", "|+", ">", ">-", ">+"):
                out[k] = text_block(ind, v)
            elif v == "":
                sub = block(ind + 1)
                # A list may start at the same indent as its key.
                if sub is None:
                    skip_blank()
                    if pos[0] < len(lines) and indent(lines[pos[0]][1]) == ind \
                            and lines[pos[0]][1].strip().startswith("-"):
                        sub = seq(ind)
                out[k] = sub
            else:
                out[k] = scalar(v, n)

    def text_block(ind, style):
        body = []
        block_ind = None
        while pos[0] < len(lines):
            n, raw = lines[pos[0]]
            if raw.strip() == "":
                body.append("")
                pos[0] += 1
                continue
            if indent(raw) <= ind:
                break
            if block_ind is None:
                block_ind = indent(raw)
            if indent(raw) < block_ind:
                raise YamlError(f"line {n}: bad indent in a text block")
            body.append(raw[block_ind:])
            pos[0] += 1
        while body and body[-1] == "":
            body.pop()
        if style.startswith(">"):
            paras, cur = [], []
            for b in body:
                if b == "":
                    paras.append(" ".join(cur))
                    cur = []
                else:
                    cur.append(b)
            paras.append(" ".join(cur))
            s = "\n".join(paras)
        else:
            s = "\n".join(body)
        return s if style.endswith("-") else s + "\n"

    return block(0)


def key_value(s):
    """Split `key: value`, or return None."""
    m = re.match(r"""^("[^"]*"|'[^']*'|[^\s"'#:][^:#]*?)\s*:(?:\s+(.*)|)$""", s)
    if not m:
        return None
    k = m.group(1)
    if k[0] in "\"'":
        k = k[1:-1]
    return k, strip_comment(m.group(2) or "")


def strip_comment(v):
    if v[:1] in "\"'[{":
        return v.strip()
    return re.sub(r"\s+#.*$", "", v).strip()


def scalar(v, n):
    v = strip_comment(v)
    if v.startswith("["):
        if not v.endswith("]"):
            raise YamlError(f"line {n}: a flow list must end on its line")
        inner = v[1:-1].strip()
        if not inner:
            return []
        return [scalar(x.strip(), n) for x in split_flow(inner, n)]
    if v.startswith("{"):
        raise YamlError(f"line {n}: flow mappings are not supported")
    if v.startswith('"'):
        if not v.endswith('"') or len(v) < 2:
            raise YamlError(f"line {n}: an unclosed string")
        return json.loads(v)
    if v.startswith("'"):
        if not v.endswith("'") or len(v) < 2:
            raise YamlError(f"line {n}: an unclosed string")
        return v[1:-1].replace("''", "'")
    if v in ("true", "True", "TRUE"):
        return True
    if v in ("false", "False", "FALSE"):
        return False
    if v in ("", "~", "null", "Null", "NULL"):
        return None
    if re.fullmatch(r"[-+]?\d+", v):
        return int(v)
    if re.fullmatch(r"[-+]?(\d+\.\d*|\.\d+)([eE][-+]?\d+)?", v):
        return float(v)
    if v[0] in "&*!%@`|>":
        raise YamlError(f"line {n}: `{v[0]}` is not supported")
    return v


def split_flow(s, n):
    out, cur, quote = [], "", None
    for c in s:
        if quote:
            cur += c
            if c == quote:
                quote = None
        elif c in "\"'":
            quote = c
            cur += c
        elif c == ",":
            out.append(cur)
            cur = ""
        elif c in "[]{}":
            raise YamlError(f"line {n}: nested flow collections are not supported")
        else:
            cur += c
    if quote:
        raise YamlError(f"line {n}: an unclosed string")
    out.append(cur)
    return out


# --------------------------------------------------------------- rules ---


def is_text(v):
    return isinstance(v, str) and v.strip() != ""


def check_form(data):
    """Return the list of problems of one issue form (empty when it is good)."""
    errs = []
    if not isinstance(data, dict):
        return ["the form must be a mapping"]
    for k in sorted(set(data) - TOP_KEYS):
        errs.append(f"unknown top-level key `{k}`")
    for k in ("name", "description"):
        if not is_text(data.get(k)):
            errs.append(f"`{k}` must be a text that is not empty")
    for k in ("labels", "assignees", "projects"):
        if k in data:
            v = data[k]
            ok = is_text(v) or (isinstance(v, list) and v and all(is_text(x) for x in v))
            if not ok:
                errs.append(f"`{k}` must be a text or a list of texts")
    if "title" in data and not isinstance(data["title"], str):
        errs.append("`title` must be a text")
    body = data.get("body")
    if not isinstance(body, list) or not body:
        errs.append("`body` must be a list that is not empty")
        return errs

    ids, labels = set(), set()
    fields = 0
    for i, item in enumerate(body):
        at = f"body[{i}]"
        if not isinstance(item, dict):
            errs.append(f"{at}: must be a mapping")
            continue
        for k in sorted(set(item) - {"type", "id", "attributes", "validations"}):
            errs.append(f"{at}: unknown key `{k}`")
        t = item.get("type")
        if t not in TYPES:
            errs.append(f"{at}: `type` must be one of {', '.join(sorted(TYPES))}, not {t!r}")
            continue
        at = f"{at} ({t})"
        if t != "markdown":
            fields += 1
        if "id" in item:
            v = item["id"]
            if not isinstance(v, str) or not ID.match(v):
                errs.append(f"{at}: `id` {v!r} may use only letters, digits, `-` and `_`")
            elif v in ids:
                errs.append(f"{at}: the id `{v}` is used two times")
            else:
                ids.add(v)
        if t == "markdown" and "id" in item:
            errs.append(f"{at}: a markdown item cannot have an `id`")
        attrs = item.get("attributes")
        if not isinstance(attrs, dict):
            errs.append(f"{at}: `attributes` must be a mapping")
            continue
        for k in sorted(set(attrs) - ATTRS[t]):
            errs.append(f"{at}: unknown attribute `{k}`")
        if t == "markdown":
            if not is_text(attrs.get("value")):
                errs.append(f"{at}: `attributes.value` must be a text that is not empty")
            if "validations" in item:
                errs.append(f"{at}: a markdown item cannot have `validations`")
            continue
        label = attrs.get("label")
        if not is_text(label):
            errs.append(f"{at}: `attributes.label` is required")
        elif label in labels:
            errs.append(f"{at}: the label `{label}` is used two times")
        else:
            labels.add(label)
        for k in ("description", "placeholder", "value", "render"):
            if k in attrs and not isinstance(attrs[k], str):
                errs.append(f"{at}: `attributes.{k}` must be a text")
        if t == "dropdown":
            errs += check_dropdown(at, attrs)
        if t == "checkboxes":
            errs += check_checkboxes(at, attrs)
        if "validations" in item:
            v = item["validations"]
            if not isinstance(v, dict) or set(v) - {"required"} \
                    or not isinstance(v.get("required", False), bool):
                errs.append(f"{at}: `validations` may have only `required: true` or `false`")
    if fields == 0:
        errs.append("`body` needs at least one field that is not markdown")
    return errs


def check_dropdown(at, attrs):
    errs = []
    opts = attrs.get("options")
    if not isinstance(opts, list) or not opts:
        return [f"{at}: `attributes.options` must be a list that is not empty"]
    seen = set()
    for o in opts:
        if not is_text(o):
            errs.append(f"{at}: option {o!r} must be a text that is not empty")
        elif o in seen:
            errs.append(f"{at}: the option `{o}` is there two times")
        elif o.strip() == "None":
            errs.append(f"{at}: `None` is a reserved option")
        else:
            seen.add(o)
    if "multiple" in attrs and not isinstance(attrs["multiple"], bool):
        errs.append(f"{at}: `attributes.multiple` must be true or false")
    if "default" in attrs:
        d = attrs["default"]
        if isinstance(d, bool) or not isinstance(d, int) or not 0 <= d < len(opts):
            errs.append(f"{at}: `attributes.default` must be the index of an option")
    return errs


def check_checkboxes(at, attrs):
    errs = []
    opts = attrs.get("options")
    if not isinstance(opts, list) or not opts:
        return [f"{at}: `attributes.options` must be a list that is not empty"]
    seen = set()
    for j, o in enumerate(opts):
        if not isinstance(o, dict) or not is_text(o.get("label")):
            errs.append(f"{at}: option {j} needs a `label`")
            continue
        if set(o) - {"label", "required"}:
            errs.append(f"{at}: option {j} may have only `label` and `required`")
        if not isinstance(o.get("required", False), bool):
            errs.append(f"{at}: option {j}: `required` must be true or false")
        if o["label"] in seen:
            errs.append(f"{at}: the option `{o['label']}` is there two times")
        seen.add(o["label"])
    return errs


def check_config(data):
    """Return the list of problems of config.yml (empty when it is good)."""
    if not isinstance(data, dict):
        return ["config.yml must be a mapping"]
    errs = []
    for k in sorted(set(data) - {"blank_issues_enabled", "contact_links"}):
        errs.append(f"unknown key `{k}`")
    if "blank_issues_enabled" in data and not isinstance(data["blank_issues_enabled"], bool):
        errs.append("`blank_issues_enabled` must be true or false")
    links = data.get("contact_links", [])
    if not isinstance(links, list):
        return errs + ["`contact_links` must be a list"]
    names = set()
    for i, link in enumerate(links):
        at = f"contact_links[{i}]"
        if not isinstance(link, dict):
            errs.append(f"{at}: must be a mapping")
            continue
        for k in sorted(set(link) - {"name", "url", "about"}):
            errs.append(f"{at}: unknown key `{k}`")
        for k in ("name", "url", "about"):
            if not is_text(link.get(k)):
                errs.append(f"{at}: `{k}` must be a text that is not empty")
        url = link.get("url")
        if isinstance(url, str) and not re.match(r"^https://[^\s/]+\.[^\s/]+(/\S*)?$", url):
            errs.append(f"{at}: `url` must be an https URL, not {url!r}")
        if link.get("name") in names:
            errs.append(f"{at}: the name `{link['name']}` is there two times")
        names.add(link.get("name"))
    return errs


def check_folder(folder):
    """Check each file of an ISSUE_TEMPLATE folder. Return {file name: problems}."""
    out = {}
    names = sorted(os.listdir(folder))
    form_names = {}
    for f in names:
        path = os.path.join(folder, f)
        if not os.path.isfile(path):
            continue
        if f.endswith(".md"):
            out[f] = []
            continue
        if not f.endswith((".yml", ".yaml")):
            out[f] = ["an issue template must end in .yml, .yaml or .md"]
            continue
        try:
            with open(path, encoding="utf-8") as fh:
                data = load_yaml(fh.read())
        except Exception as e:  # noqa: BLE001 - any parse error is a problem of the file
            out[f] = [f"the YAML does not parse: {e}"]
            continue
        if f in ("config.yml", "config.yaml"):
            out[f] = check_config(data)
            continue
        errs = check_form(data)
        name = data.get("name") if isinstance(data, dict) else None
        if isinstance(name, str):
            if name in form_names:
                errs.append(f"the name `{name}` is used by {form_names[name]} too")
            form_names[name] = f
        out[f] = errs
    if not any(f not in ("config.yml", "config.yaml") and not f.endswith(".md") for f in out):
        out.setdefault("(folder)", []).append("there is no issue form")
    return out


def main(argv):
    folder = argv[1] if len(argv) > 1 else FORMS
    result = check_folder(folder)
    bad = 0
    for f, errs in result.items():
        if errs:
            bad += 1
            print(f"{f}:")
            for e in errs:
                print(f"  - {e}")
        else:
            print(f"{f}: ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

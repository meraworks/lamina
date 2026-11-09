#!/usr/bin/env python3
"""Tests of scripts/check_issue_forms.py.

    python3 -m unittest discover -s scripts -p 'test_*.py' -v
"""
import copy
import os
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_issue_forms as c  # noqa: E402

try:
    import yaml
except ImportError:
    yaml = None

GOOD = {
    "name": "Bug report",
    "description": "Something is wrong.",
    "labels": ["bug"],
    "body": [
        {"type": "markdown", "attributes": {"value": "Thank you."}},
        {"type": "textarea", "id": "what", "attributes": {"label": "What happened?"},
         "validations": {"required": True}},
        {"type": "input", "id": "version", "attributes": {"label": "Version"}},
        {"type": "dropdown", "id": "os",
         "attributes": {"label": "OS", "options": ["Windows", "macOS", "Linux"]}},
        {"type": "checkboxes", "id": "terms",
         "attributes": {"label": "Checks", "options": [{"label": "I searched", "required": True}]}},
    ],
}

GOOD_CONFIG = {
    "blank_issues_enabled": False,
    "contact_links": [{"name": "Guide", "url": "https://example.com/guide", "about": "Read it."}],
}


def form(edit):
    f = copy.deepcopy(GOOD)
    edit(f)
    return c.check_form(f)


def config(edit):
    f = copy.deepcopy(GOOD_CONFIG)
    edit(f)
    return c.check_config(f)


class TheRepoForms(unittest.TestCase):
    def test_every_file_passes(self):
        result = c.check_folder(c.FORMS)
        self.assertEqual({f: e for f, e in result.items() if e}, {})

    def test_the_expected_forms_are_there(self):
        names = set(os.listdir(c.FORMS))
        for f in ("bug-report.yml", "board-import.yml", "feature-request.yml", "config.yml"):
            self.assertIn(f, names)

    def test_the_mini_reader_passes_too(self):
        for f in sorted(os.listdir(c.FORMS)):
            with open(os.path.join(c.FORMS, f), encoding="utf-8") as fh:
                data = c.mini_yaml(fh.read())
            check = c.check_config if f == "config.yml" else c.check_form
            self.assertEqual(check(data), [], f)

    @unittest.skipIf(yaml is None, "PyYAML is not installed")
    def test_the_mini_reader_agrees_with_pyyaml(self):
        for f in sorted(os.listdir(c.FORMS)):
            with open(os.path.join(c.FORMS, f), encoding="utf-8") as fh:
                text = fh.read()
            self.assertEqual(c.mini_yaml(text), yaml.safe_load(text), f)

    def test_bug_report_asks_what_the_issue_asks(self):
        with open(os.path.join(c.FORMS, "bug-report.yml"), encoding="utf-8") as fh:
            data = c.load_yaml(fh.read())
        ids = {i.get("id") for i in data["body"]}
        for want in ("what-happened", "expected", "actual", "version", "os", "mcp-client",
                     "board", "logs"):
            self.assertIn(want, ids)
        text = str(data)
        self.assertIn("lamina --version", text)

    def test_board_import_lists_the_formats(self):
        with open(os.path.join(c.FORMS, "board-import.yml"), encoding="utf-8") as fh:
            data = c.load_yaml(fh.read())
        fmt = next(i for i in data["body"] if i.get("id") == "format")
        joined = " ".join(fmt["attributes"]["options"])
        for want in ("KiCad", "EasyEDA", "Gerber"):
            self.assertIn(want, joined)

    def test_config_links_the_guide_and_quick_start(self):
        with open(os.path.join(c.FORMS, "config.yml"), encoding="utf-8") as fh:
            data = c.load_yaml(fh.read())
        urls = [link["url"] for link in data["contact_links"]]
        self.assertIn("https://github.com/meraworks/lamina/blob/master/docs/guide.md", urls)
        self.assertTrue(any(u.endswith("#quick-start") for u in urls))
        self.assertFalse(any("discussions" in u for u in urls))


class GoodFormPasses(unittest.TestCase):
    def test_good(self):
        self.assertEqual(c.check_form(GOOD), [])

    def test_labels_as_text(self):
        self.assertEqual(form(lambda f: f.update(labels="bug, import")), [])

    def test_dropdown_default_and_multiple(self):
        def edit(f):
            f["body"][3]["attributes"].update(default=2, multiple=True)
        self.assertEqual(form(edit), [])


class BrokenFormFails(unittest.TestCase):
    def assertFails(self, errs, part):
        self.assertTrue(errs, "the broken form passed")
        self.assertTrue(any(part in e for e in errs), f"no error with {part!r}: {errs}")

    def test_not_a_mapping(self):
        self.assertFails(c.check_form(["a"]), "mapping")
        self.assertFails(c.check_form(None), "mapping")

    def test_missing_name(self):
        self.assertFails(form(lambda f: f.pop("name")), "`name`")

    def test_empty_description(self):
        self.assertFails(form(lambda f: f.update(description="  ")), "`description`")

    def test_missing_body(self):
        self.assertFails(form(lambda f: f.pop("body")), "`body`")

    def test_empty_body(self):
        self.assertFails(form(lambda f: f.update(body=[])), "`body`")

    def test_only_markdown(self):
        self.assertFails(form(lambda f: f.update(body=f["body"][:1])), "not markdown")

    def test_unknown_top_key(self):
        self.assertFails(form(lambda f: f.update(about="x")), "unknown top-level key `about`")

    def test_bad_labels(self):
        self.assertFails(form(lambda f: f.update(labels=[1])), "`labels`")
        self.assertFails(form(lambda f: f.update(labels=[])), "`labels`")

    def test_unknown_type(self):
        self.assertFails(form(lambda f: f["body"][1].update(type="text")), "`type`")

    def test_missing_type(self):
        self.assertFails(form(lambda f: f["body"][1].pop("type")), "`type`")

    def test_unknown_item_key(self):
        self.assertFails(form(lambda f: f["body"][1].update(label="x")), "unknown key `label`")

    def test_bad_id(self):
        for bad in ("has space", "dot.id", "ümlaut", "", 5):
            self.assertFails(form(lambda f: f["body"][1].update(id=bad)), "`id`")

    def test_good_ids(self):
        for good in ("a", "A-1", "x_y", "123"):
            self.assertEqual(form(lambda f: f["body"][1].update(id=good)), [], good)

    def test_duplicate_id(self):
        self.assertFails(form(lambda f: f["body"][2].update(id="what")), "two times")

    def test_markdown_with_id(self):
        self.assertFails(form(lambda f: f["body"][0].update(id="intro")), "markdown")

    def test_markdown_without_value(self):
        self.assertFails(form(lambda f: f["body"][0].update(attributes={})), "`attributes.value`")

    def test_markdown_with_validations(self):
        self.assertFails(form(lambda f: f["body"][0].update(validations={"required": True})),
                         "markdown")

    def test_missing_attributes(self):
        self.assertFails(form(lambda f: f["body"][1].pop("attributes")), "`attributes`")

    def test_missing_label(self):
        for i in (1, 2, 3, 4):
            self.assertFails(form(lambda f: f["body"][i]["attributes"].pop("label")),
                             "`attributes.label`")

    def test_duplicate_label(self):
        self.assertFails(form(lambda f: f["body"][2]["attributes"].update(label="What happened?")),
                         "two times")

    def test_unknown_attribute(self):
        self.assertFails(form(lambda f: f["body"][2]["attributes"].update(render="text")),
                         "unknown attribute `render`")

    def test_dropdown_without_options(self):
        self.assertFails(form(lambda f: f["body"][3]["attributes"].pop("options")), "options")
        self.assertFails(form(lambda f: f["body"][3]["attributes"].update(options=[])), "options")

    def test_dropdown_duplicate_option(self):
        self.assertFails(form(lambda f: f["body"][3]["attributes"].update(options=["A", "A"])),
                         "two times")

    def test_dropdown_empty_option(self):
        self.assertFails(form(lambda f: f["body"][3]["attributes"].update(options=["A", ""])),
                         "option")

    def test_dropdown_none_option(self):
        self.assertFails(form(lambda f: f["body"][3]["attributes"].update(options=["A", "None"])),
                         "reserved")

    def test_dropdown_bad_default(self):
        for bad in (3, -1, "0", True):
            self.assertFails(form(lambda f: f["body"][3]["attributes"].update(default=bad)),
                             "`attributes.default`")

    def test_checkbox_without_label(self):
        self.assertFails(form(lambda f: f["body"][4]["attributes"].update(options=[{}])),
                         "`label`")

    def test_checkbox_bad_required(self):
        def edit(f):
            f["body"][4]["attributes"]["options"][0]["required"] = "yes"
        self.assertFails(form(edit), "`required`")

    def test_bad_validations(self):
        self.assertFails(form(lambda f: f["body"][1].update(validations={"required": "yes"})),
                         "`validations`")
        self.assertFails(form(lambda f: f["body"][1].update(validations={"min": 1})),
                         "`validations`")


class Config(unittest.TestCase):
    def assertFails(self, errs, part):
        self.assertTrue(errs, "the broken config passed")
        self.assertTrue(any(part in e for e in errs), f"no error with {part!r}: {errs}")

    def test_good(self):
        self.assertEqual(c.check_config(GOOD_CONFIG), [])
        self.assertEqual(c.check_config({"blank_issues_enabled": True}), [])

    def test_not_a_mapping(self):
        self.assertFails(c.check_config(None), "mapping")

    def test_blank_issues_not_bool(self):
        self.assertFails(config(lambda f: f.update(blank_issues_enabled="yes")), "true or false")

    def test_unknown_key(self):
        self.assertFails(config(lambda f: f.update(links=[])), "unknown key `links`")

    def test_links_not_list(self):
        self.assertFails(config(lambda f: f.update(contact_links={})), "list")

    def test_missing_link_keys(self):
        for k in ("name", "url", "about"):
            self.assertFails(config(lambda f: f["contact_links"][0].pop(k)), f"`{k}`")

    def test_not_https(self):
        for bad in ("http://example.com", "example.com", "https://", "https://exa mple.com",
                    "ftp://example.com/x"):
            self.assertFails(config(lambda f: f["contact_links"][0].update(url=bad)), "https")

    def test_duplicate_link_name(self):
        def edit(f):
            f["contact_links"].append(dict(f["contact_links"][0]))
        self.assertFails(config(edit), "two times")


class Folder(unittest.TestCase):
    def write(self, files):
        d = tempfile.mkdtemp()
        self.addCleanup(lambda: [os.remove(os.path.join(d, f)) for f in os.listdir(d)]
                        or os.rmdir(d))
        for name, text in files.items():
            with open(os.path.join(d, name), "w", encoding="utf-8") as fh:
                fh.write(text)
        return c.check_folder(d)

    FORM = "name: {name}\ndescription: d\nbody:\n  - type: input\n    attributes:\n" \
           "      label: L\n"

    def test_good_folder(self):
        r = self.write({"a.yml": self.FORM.format(name="A"),
                        "config.yml": "blank_issues_enabled: false\n"})
        self.assertEqual(r, {"a.yml": [], "config.yml": []})

    def test_parse_error(self):
        r = self.write({"a.yml": "name: [unclosed\n"})
        self.assertTrue(any("parse" in e for e in r["a.yml"]))

    def test_wrong_extension(self):
        r = self.write({"a.yml": self.FORM.format(name="A"), "b.txt": "x"})
        self.assertTrue(r["b.txt"])

    def test_duplicate_form_names(self):
        r = self.write({"a.yml": self.FORM.format(name="A"), "b.yml": self.FORM.format(name="A")})
        self.assertTrue(any("used by a.yml" in e for e in r["b.yml"]))

    def test_no_forms(self):
        r = self.write({"config.yml": "blank_issues_enabled: true\n"})
        self.assertTrue(r["(folder)"])

    def test_main_exit_code(self):
        d = tempfile.mkdtemp()
        with open(os.path.join(d, "a.yml"), "w") as fh:
            fh.write("name: A\nbody: []\n")
        with open(os.devnull, "w") as null:
            out, sys.stdout = sys.stdout, null
            try:
                self.assertEqual(c.main(["x", d]), 1)
                self.assertEqual(c.main(["x", c.FORMS]), 0)
            finally:
                sys.stdout = out
        os.remove(os.path.join(d, "a.yml"))
        os.rmdir(d)


class MiniYaml(unittest.TestCase):
    def test_scalars(self):
        self.assertEqual(
            c.mini_yaml('a: 1\nb: true\nc: "x: y"\nd: \'it\'\'s\'\ne: plain text # note\nf:\n'),
            {"a": 1, "b": True, "c": "x: y", "d": "it's", "e": "plain text", "f": None})

    def test_flow_list(self):
        self.assertEqual(c.mini_yaml('labels: ["bug", triage]\nx: []\n'),
                         {"labels": ["bug", "triage"], "x": []})

    def test_literal_and_folded_blocks(self):
        text = "a: |\n  one\n\n  two\nb: >\n  three\n  four\nc: |-\n  five\n"
        self.assertEqual(c.mini_yaml(text), {"a": "one\n\ntwo\n", "b": "three four\n", "c": "five"})

    def test_nested_lists(self):
        text = "body:\n  - type: a\n    attributes:\n      options:\n        - x\n        - y\n" \
               "  - type: b\nlist:\n- 1\n- 2\n"
        self.assertEqual(c.mini_yaml(text), {
            "body": [{"type": "a", "attributes": {"options": ["x", "y"]}}, {"type": "b"}],
            "list": [1, 2]})

    def test_rejects(self):
        for bad in ("a: 1\na: 2\n", "a:\n\t- x\n", "a: {b: 1}\n", "a: [x\n", "a: &anchor x\n",
                    "a: 1\n   b: 2\n", "just text\n", 'a: "open\n'):
            with self.assertRaises(c.YamlError, msg=bad):
                c.mini_yaml(bad)

    @unittest.skipIf(yaml is None, "PyYAML is not installed")
    def test_agrees_with_pyyaml(self):
        text = "name: N\ndescription: 'D'\nlabels: [\"a\", b]\nbody:\n  - type: markdown\n" \
               "    attributes:\n      value: |\n        Hi.\n        `code` here.\n" \
               "  - type: dropdown\n    id: d\n    attributes:\n      label: L\n" \
               "      options:\n        - Windows\n        - Other (say which)\n" \
               "      default: 0\n    validations:\n      required: false\n"
        self.assertEqual(c.mini_yaml(text), yaml.safe_load(text))


if __name__ == "__main__":
    unittest.main()

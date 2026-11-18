import sys

from tests.layers import TOOLS
sys.path.insert(0, str(TOOLS))
from corpus_leak import Corpus, norm_lines  # noqa: E402

SCRIPT = """rem a long comment written by the mod's authors, long enough to count on its own line
ObjectTemplate.create Bundle Jeep
ObjectTemplate.setNetworkableInfo BundleInfo
ObjectTemplate.addTemplate JeepBody
ObjectTemplate.setPosition 0/1.2/0.5
ObjectTemplate.setRotation 0/0/0
endIf
"""


def corpus(tmp_path, common=20):
    for i in range(3):
        (tmp_path / f"a{i}.con").write_text("endIf\nObjectTemplate.setNetworkableInfo BundleInfo\n")
    (tmp_path / "jeep.con").write_text(SCRIPT)
    return Corpus(sorted(tmp_path.glob("*.con")), k=3, common=common)


def test_norm_strips_markers_and_splits_escaped_newlines():
    assert norm_lines('    //! `ObjectTemplate.create  Bundle Jeep`\n"a\\nObjectTemplate.addTemplate X\\n",') == [
        (1, "objecttemplate.create bundle jeep"), (2, "objecttemplate.addtemplate x")]


def test_a_copied_block_in_a_comment_or_a_string_is_found(tmp_path):
    c = corpus(tmp_path, common=3)
    rust = "// ObjectTemplate.create Bundle Jeep\n// ObjectTemplate.addTemplate JeepBody\n// ObjectTemplate.setPosition 0/1.2/0.5\n"
    assert [k for _, k, _ in c.hits(rust, 80)] == ["run"]
    # the first line, glued to `let s = "`, is lost; the block is still found from the next ones
    lit = 'let s = "ObjectTemplate.create Bundle Jeep\\nObjectTemplate.addTemplate JeepBody\\nObjectTemplate.setPosition 0/1.2/0.5\\nObjectTemplate.setRotation 0/0/0";'
    assert [k for _, k, _ in c.hits(lit, 80)] == ["run"]


def test_boilerplate_and_short_lines_alone_are_not_hits(tmp_path):
    c = corpus(tmp_path, common=3)
    text = "ObjectTemplate.create Bundle Jeep\nendIf\nObjectTemplate.setNetworkableInfo BundleInfo\nsomething of ours\n"
    assert c.hits(text, 80) == []
    # but boilerplate between two copied lines does not hide the block
    text = "ObjectTemplate.create Bundle Jeep\nObjectTemplate.setNetworkableInfo BundleInfo\nObjectTemplate.addTemplate JeepBody\nObjectTemplate.setPosition 0/1.2/0.5\n"
    assert [k for _, k, _ in c.hits(text, 80)] == ["run"]


def test_a_long_line_counts_alone(tmp_path):
    c = corpus(tmp_path)
    hits = c.hits("# a long comment written by the mod's authors, long enough to count on its own line\n", 60)
    assert [(n, k) for n, k, _ in hits] == [(1, "long")]

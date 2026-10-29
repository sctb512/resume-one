"""Data-only font conversion regression checks; requires fonttools[woff]."""
import base64
import io
import json
from pathlib import Path
import subprocess
import struct
import sys
import tempfile
import unittest
from fontTools.ttLib import TTFont

TOOL = Path(__file__).with_name("convert-embedded-fonts.py")
FIXTURE = TOOL.parents[1].parent / "crates/resume-one-core/tests/fixtures/theme-font.ttf"


class ConversionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="resume-one-font-conversion-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "faces.js"
        self.output = self.root / "output"

    def source_with(self, content):
        encoded = base64.b64encode(content).decode("ascii")
        self.source.write_text("{weight:400,style:'normal',data:'" + encoded + "'}", encoding="utf8")

    def run_tool(self):
        return subprocess.run([sys.executable, str(TOOL), "--input", str(self.source),
                               "--output", str(self.output), "--family", "Resume One Converter Test"],
                              capture_output=True, text=True, timeout=30)

    def test_static_face_preserves_outlines_and_license_but_renames_family(self):
        original = TTFont(FIXTURE, recalcTimestamp=False)
        original.flavor = "woff2"
        buffer = io.BytesIO()
        original.save(buffer)
        self.source_with(buffer.getvalue())
        result = self.run_tool()
        self.assertEqual(result.returncode, 0, result.stderr)
        converted = TTFont(self.output / "normal-400.ttf")
        self.assertNotIn("fvar", converted)
        self.assertEqual(converted["name"].getDebugName(1), "Resume One Converter Test")
        self.assertEqual(converted["name"].getDebugName(13), original["name"].getDebugName(13))
        self.assertEqual(converted.getGlyphOrder(), original.getGlyphOrder())
        self.assertEqual(converted["OS/2"].fsType, original["OS/2"].fsType)
        sentinel = self.output / "keep.txt"
        sentinel.write_text("keep", encoding="utf8")
        self.assertNotEqual(self.run_tool().returncode, 0)
        self.assertEqual(sentinel.read_text(encoding="utf8"), "keep")

    def test_oversized_decoded_header_is_rejected_before_font_parser(self):
        header = bytearray(48)
        header[:4] = b"wOF2"
        header[16:20] = (8 * 1024 * 1024 + 1).to_bytes(4, "big")
        self.source_with(header)
        result = self.run_tool()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("before parsing", result.stderr)
        self.assertFalse(self.output.exists())

    def test_static_face_alias_preserves_every_non_naming_table_and_license(self):
        result = subprocess.run([sys.executable,str(TOOL),"--static-font",str(FIXTURE),"--output",str(self.output),"--family","Resume One Static W400"],capture_output=True,text=True,timeout=30)
        self.assertEqual(result.returncode,0,result.stderr)
        metadata=json.loads(result.stdout)
        original=TTFont(FIXTURE,recalcTimestamp=False)
        path=self.output/metadata["faces"][0]["path"]
        renamed=TTFont(path,recalcTimestamp=False)
        self.assertEqual(renamed["name"].getDebugName(1),"Resume One Static W400")
        self.assertEqual(renamed["name"].getDebugName(16),"Resume One Static W400")
        self.assertEqual(renamed["OS/2"].fsType,original["OS/2"].fsType)
        for tag in original.reader.keys():
            if tag not in ("name","head"):
                self.assertEqual(original.reader[tag],renamed.reader[tag],tag)
        a,b=original.reader["head"],renamed.reader["head"]
        self.assertEqual(a[:8]+a[12:],b[:8]+b[12:])
        raw=path.read_bytes()
        padded=raw+b"\0"*((-len(raw))%4)
        self.assertEqual(sum(struct.unpack(f">{len(padded)//4}I",padded)) & 0xFFFFFFFF,0xB1B0AFBA)
        for name_id in (0,7,8,9,10,11,12,13,14):
            self.assertEqual(original["name"].getDebugName(name_id),renamed["name"].getDebugName(name_id))

    def test_static_collection_or_existing_output_never_overwrites(self):
        for signature in (b"ttcf",b"OTTO"):
            self.source.write_bytes(signature+b"\0"*100)
            result=subprocess.run([sys.executable,str(TOOL),"--static-font",str(self.source),"--output",str(self.output),"--family","Resume One Static W400"],capture_output=True,text=True,timeout=30)
            self.assertNotEqual(result.returncode,0)
            self.assertIn("standalone TrueType",result.stderr)
            self.assertFalse(self.output.exists())

    def test_legacy_full_name_and_variation_prefix_cannot_keep_old_identity(self):
        source=self.root/"static.ttf"
        font=TTFont(FIXTURE,recalcTimestamp=False)
        for name_id in (18,21,25): font["name"].setName("Old Identity",name_id,3,1,0x409)
        font.save(source)
        result=subprocess.run([sys.executable,str(TOOL),"--static-font",str(source),"--output",str(self.output),"--family","Resume One Static W400"],capture_output=True,text=True,timeout=30)
        self.assertEqual(result.returncode,0,result.stderr)
        renamed=TTFont(self.output/"normal-400.ttf")
        self.assertEqual(renamed["name"].getDebugName(18),"Resume One Static W400 Regular")
        self.assertEqual(renamed["name"].getDebugName(21),"Resume One Static W400")
        self.assertEqual(renamed["name"].getDebugName(25),"ResumeOneStaticW400Regular")


if __name__ == "__main__":
    unittest.main()

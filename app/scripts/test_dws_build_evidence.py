import copy
import hashlib
import unittest
from dws_build_evidence import compare


class BuildEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.lock = {'commit': 'pinned', 'goVersion': 'go1.27.1', 'patchSha256': 'patch'}

    def payload(self, binary):
        digest = hashlib.sha256(binary).hexdigest()
        return [binary, {'binarySha256': digest, 'goVersion': 'go1.27.1', 'modules': [{'path': 'dependency', 'version': 'v1'}]},
                b'license evidence', {**self.lock, 'binarySha256': digest}]

    def test_native_toolchains_can_produce_different_bytes_with_matching_inputs(self):
        compare(self.payload(b'clang'), self.payload(b'gcc'), self.lock)

    def test_changed_binary_without_matching_evidence_fails(self):
        changed = self.payload(b'old'); changed[0] = b'tampered'
        with self.assertRaises(ValueError):
            compare(changed, self.payload(b'rebuilt'), self.lock)

    def test_changed_dependency_license_source_or_toolchain_fails(self):
        for field in ['dependency', 'license', 'source', 'toolchain', 'provenanceHash']:
            with self.subTest(field=field):
                changed = copy.deepcopy(self.payload(b'rebuilt'))
                if field == 'dependency': changed[1]['modules'][0]['version'] = 'v2'
                if field == 'license': changed[2] = b'different license'
                if field == 'source': changed[3]['commit'] = 'unpinned'
                if field == 'toolchain': changed[1]['goVersion'] = 'go1.0'
                if field == 'provenanceHash': changed[3]['binarySha256'] = 'wrong'
                with self.assertRaises(ValueError):
                    compare(self.payload(b'bundled'), changed, self.lock)


if __name__ == '__main__':
    unittest.main()

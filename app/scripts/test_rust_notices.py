#!/usr/bin/env python3
"""Isolated fake-tool/cache tests; no application, compiler build, or credentials."""
import copy
import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('rust_notices', Path(__file__).with_name('rust-notices.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

MIT = '''MIT License

Copyright (c) 2026 Fixture Authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
'''
# A synthetic structural fixture only, never published as legal material.
APACHE_MARKERS = '''Apache License Version 2.0
1. Definitions
2. Grant of Copyright License
3. Grant of Patent License
4. Redistribution
5. Submission of Contributions
6. Trademarks
7. Disclaimer of Warranty
8. Limitation of Liability
9. Accepting Warranty or Additional Liability
END OF TERMS AND CONDITIONS
'''
# Authenticated archive license originals, retained as portable parser fixtures.
LEGAL_FIXTURE_PROVENANCE = [{'fixture': 'CC0-1.0.txt', 'kind': 'authenticated-current-crate-full-license-positive-test', 'archive': 'dunce-1.0.5.crate', 'sourcePath': 'dunce-1.0.5/LICENSE', 'sourceUrl': 'https://static.crates.io/crates/dunce/dunce-1.0.5.crate', 'archiveSha256': '92773504d58c093f6de2459af4af33faa518c13451eb8f2b5698ed3d36e7c813', 'sha256': 'a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499'}, {'fixture': 'Unicode-3.0.txt', 'kind': 'authenticated-current-crate-full-license-positive-test', 'archive': 'icu_collections-2.2.0.crate', 'sourcePath': 'icu_collections-2.2.0/LICENSE', 'sourceUrl': 'https://static.crates.io/crates/icu_collections/icu_collections-2.2.0.crate', 'archiveSha256': '2984d1cd16c883d7935b9e07e44071dca8d917fd52ecc02c04d5fa0b5a3f191c', 'sha256': 'f367c1b8e1aa262435251e442901da4607b4650e0e63a026f5044473ecfb90f2'}, {'fixture': 'CDLA-Permissive-2.0.txt', 'kind': 'authenticated-current-crate-full-license-positive-test', 'archive': 'webpki-roots-1.0.7.crate', 'sourcePath': 'webpki-roots-1.0.7/LICENSE', 'sourceUrl': 'https://static.crates.io/crates/webpki-roots/webpki-roots-1.0.7.crate', 'archiveSha256': '52f5ee44c96cf55f1b349600768e3ece3a8f26010c05265ab73f945bb1a2eb9d', 'sha256': 'e271993808fec50ab29350b39539cdec611a9103f827e0aa26d61da70e2d33f8'}, {'fixture': 'Apache-2.0.txt', 'kind': 'authenticated-current-crate-full-license-positive-test', 'archive': 'unicode-ident-1.0.24.crate', 'sourcePath': 'unicode-ident-1.0.24/LICENSE-APACHE', 'sourceUrl': 'https://static.crates.io/crates/unicode-ident/unicode-ident-1.0.24.crate', 'archiveSha256': 'e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75', 'sha256': '62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a'}]
LEGAL_FIXTURES = {'CC0-1.0.txt': 'Creative Commons Legal Code\n\nCC0 1.0 Universal\n\n    CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM AND DOES NOT PROVIDE\n    LEGAL SERVICES. DISTRIBUTION OF THIS DOCUMENT DOES NOT CREATE AN\n    ATTORNEY-CLIENT RELATIONSHIP. CREATIVE COMMONS PROVIDES THIS\n    INFORMATION ON AN "AS-IS" BASIS. CREATIVE COMMONS MAKES NO WARRANTIES\n    REGARDING THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS\n    PROVIDED HEREUNDER, AND DISCLAIMS LIABILITY FOR DAMAGES RESULTING FROM\n    THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS PROVIDED\n    HEREUNDER.\n\nStatement of Purpose\n\nThe laws of most jurisdictions throughout the world automatically confer\nexclusive Copyright and Related Rights (defined below) upon the creator\nand subsequent owner(s) (each and all, an "owner") of an original work of\nauthorship and/or a database (each, a "Work").\n\nCertain owners wish to permanently relinquish those rights to a Work for\nthe purpose of contributing to a commons of creative, cultural and\nscientific works ("Commons") that the public can reliably and without fear\nof later claims of infringement build upon, modify, incorporate in other\nworks, reuse and redistribute as freely as possible in any form whatsoever\nand for any purposes, including without limitation commercial purposes.\nThese owners may contribute to the Commons to promote the ideal of a free\nculture and the further production of creative, cultural and scientific\nworks, or to gain reputation or greater distribution for their Work in\npart through the use and efforts of others.\n\nFor these and/or other purposes and motivations, and without any\nexpectation of additional consideration or compensation, the person\nassociating CC0 with a Work (the "Affirmer"), to the extent that he or she\nis an owner of Copyright and Related Rights in the Work, voluntarily\nelects to apply CC0 to the Work and publicly distribute the Work under its\nterms, with knowledge of his or her Copyright and Related Rights in the\nWork and the meaning and intended legal effect of CC0 on those rights.\n\n1. Copyright and Related Rights. A Work made available under CC0 may be\nprotected by copyright and related or neighboring rights ("Copyright and\nRelated Rights"). Copyright and Related Rights include, but are not\nlimited to, the following:\n\n  i. the right to reproduce, adapt, distribute, perform, display,\n     communicate, and translate a Work;\n ii. moral rights retained by the original author(s) and/or performer(s);\niii. publicity and privacy rights pertaining to a person\'s image or\n     likeness depicted in a Work;\n iv. rights protecting against unfair competition in regards to a Work,\n     subject to the limitations in paragraph 4(a), below;\n  v. rights protecting the extraction, dissemination, use and reuse of data\n     in a Work;\n vi. database rights (such as those arising under Directive 96/9/EC of the\n     European Parliament and of the Council of 11 March 1996 on the legal\n     protection of databases, and under any national implementation\n     thereof, including any amended or successor version of such\n     directive); and\nvii. other similar, equivalent or corresponding rights throughout the\n     world based on applicable law or treaty, and any national\n     implementations thereof.\n\n2. Waiver. To the greatest extent permitted by, but not in contravention\nof, applicable law, Affirmer hereby overtly, fully, permanently,\nirrevocably and unconditionally waives, abandons, and surrenders all of\nAffirmer\'s Copyright and Related Rights and associated claims and causes\nof action, whether now known or unknown (including existing as well as\nfuture claims and causes of action), in the Work (i) in all territories\nworldwide, (ii) for the maximum duration provided by applicable law or\ntreaty (including future time extensions), (iii) in any current or future\nmedium and for any number of copies, and (iv) for any purpose whatsoever,\nincluding without limitation commercial, advertising or promotional\npurposes (the "Waiver"). Affirmer makes the Waiver for the benefit of each\nmember of the public at large and to the detriment of Affirmer\'s heirs and\nsuccessors, fully intending that such Waiver shall not be subject to\nrevocation, rescission, cancellation, termination, or any other legal or\nequitable action to disrupt the quiet enjoyment of the Work by the public\nas contemplated by Affirmer\'s express Statement of Purpose.\n\n3. Public License Fallback. Should any part of the Waiver for any reason\nbe judged legally invalid or ineffective under applicable law, then the\nWaiver shall be preserved to the maximum extent permitted taking into\naccount Affirmer\'s express Statement of Purpose. In addition, to the\nextent the Waiver is so judged Affirmer hereby grants to each affected\nperson a royalty-free, non transferable, non sublicensable, non exclusive,\nirrevocable and unconditional license to exercise Affirmer\'s Copyright and\nRelated Rights in the Work (i) in all territories worldwide, (ii) for the\nmaximum duration provided by applicable law or treaty (including future\ntime extensions), (iii) in any current or future medium and for any number\nof copies, and (iv) for any purpose whatsoever, including without\nlimitation commercial, advertising or promotional purposes (the\n"License"). The License shall be deemed effective as of the date CC0 was\napplied by Affirmer to the Work. Should any part of the License for any\nreason be judged legally invalid or ineffective under applicable law, such\npartial invalidity or ineffectiveness shall not invalidate the remainder\nof the License, and in such case Affirmer hereby affirms that he or she\nwill not (i) exercise any of his or her remaining Copyright and Related\nRights in the Work or (ii) assert any associated claims and causes of\naction with respect to the Work, in either case contrary to Affirmer\'s\nexpress Statement of Purpose.\n\n4. Limitations and Disclaimers.\n\n a. No trademark or patent rights held by Affirmer are waived, abandoned,\n    surrendered, licensed or otherwise affected by this document.\n b. Affirmer offers the Work as-is and makes no representations or\n    warranties of any kind concerning the Work, express, implied,\n    statutory or otherwise, including without limitation warranties of\n    title, merchantability, fitness for a particular purpose, non\n    infringement, or the absence of latent or other defects, accuracy, or\n    the present or absence of errors, whether or not discoverable, all to\n    the greatest extent permissible under applicable law.\n c. Affirmer disclaims responsibility for clearing rights of other persons\n    that may apply to the Work or any use thereof, including without\n    limitation any person\'s Copyright and Related Rights in the Work.\n    Further, Affirmer disclaims responsibility for obtaining any necessary\n    consents, permissions or other rights required for any use of the\n    Work.\n d. Affirmer understands and acknowledges that Creative Commons is not a\n    party to this document and has no duty or obligation with respect to\n    this CC0 or use of the Work.\n', 'Unicode-3.0.txt': 'UNICODE LICENSE V3\n\nCOPYRIGHT AND PERMISSION NOTICE\n\nCopyright © 2020-2024 Unicode, Inc.\n\nNOTICE TO USER: Carefully read the following legal agreement. BY\nDOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR\nSOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE\nTERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT\nDOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.\n\nPermission is hereby granted, free of charge, to any person obtaining a\ncopy of data files and any associated documentation (the "Data Files") or\nsoftware and any associated documentation (the "Software") to deal in the\nData Files or Software without restriction, including without limitation\nthe rights to use, copy, modify, merge, publish, distribute, and/or sell\ncopies of the Data Files or Software, and to permit persons to whom the\nData Files or Software are furnished to do so, provided that either (a)\nthis copyright and permission notice appear with all copies of the Data\nFiles or Software, or (b) this copyright and permission notice appear in\nassociated Documentation.\n\nTHE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY\nKIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF\nMERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF\nTHIRD PARTY RIGHTS.\n\nIN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE\nBE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,\nOR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,\nWHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,\nARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA\nFILES OR SOFTWARE.\n\nExcept as contained in this notice, the name of a copyright holder shall\nnot be used in advertising or otherwise to promote the sale, use or other\ndealings in these Data Files or Software without prior written\nauthorization of the copyright holder.\n\nSPDX-License-Identifier: Unicode-3.0\n\n—\n\nPortions of ICU4X may have been adapted from ICU4C and/or ICU4J.\nICU 1.8.1 to ICU 57.1 © 1995-2016 International Business Machines Corporation and others.\n', 'CDLA-Permissive-2.0.txt': '# Community Data License Agreement - Permissive - Version 2.0\n\nThis is the Community Data License Agreement - Permissive, Version\n2.0 (the "agreement"). Data Provider(s) and Data Recipient(s) agree\nas follows:\n\n## 1. Provision of the Data\n\n1.1. A Data Recipient may use, modify, and share the Data made\navailable by Data Provider(s) under this agreement if that Data\nRecipient follows the terms of this agreement.\n\n1.2. This agreement does not impose any restriction on a Data\nRecipient\'s use, modification, or sharing of any portions of the\nData that are in the public domain or that may be used, modified,\nor shared under any other legal exception or limitation.\n\n## 2. Conditions for Sharing Data\n\n2.1. A Data Recipient may share Data, with or without modifications, so\nlong as the Data Recipient makes available the text of this agreement\nwith the shared Data.\n\n## 3. No Restrictions on Results\n\n3.1. This agreement does not impose any restriction or obligations\nwith respect to the use, modification, or sharing of Results.\n\n## 4. No Warranty; Limitation of Liability\n\n4.1. All Data Recipients receive the Data subject to the following\nterms:\n\nTHE DATA IS PROVIDED ON AN "AS IS" BASIS, WITHOUT REPRESENTATIONS,\nWARRANTIES OR CONDITIONS OF ANY KIND, EITHER EXPRESS OR IMPLIED\nINCLUDING, WITHOUT LIMITATION, ANY WARRANTIES OR CONDITIONS OF TITLE,\nNON-INFRINGEMENT, MERCHANTABILITY OR FITNESS FOR A PARTICULAR PURPOSE.\n\nNO DATA PROVIDER SHALL HAVE ANY LIABILITY FOR ANY DIRECT, INDIRECT,\nINCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING\nWITHOUT LIMITATION LOST PROFITS), HOWEVER CAUSED AND ON ANY THEORY OF\nLIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING\nNEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE DATA OR RESULTS,\nEVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGES.\n\n## 5. Definitions\n\n5.1. "Data" means the material received by a Data Recipient under\nthis agreement.\n\n5.2. "Data Provider" means any person who is the source of Data\nprovided under this agreement and in reliance on a Data Recipient\'s\nagreement to its terms.\n\n5.3. "Data Recipient" means any person who receives Data directly\nor indirectly from a Data Provider and agrees to the terms of this\nagreement.\n\n5.4. "Results" means any outcome obtained by computational analysis\nof Data, including for example machine learning models and models\'\ninsights.\n', 'Apache-2.0.txt': '                              Apache License\n                        Version 2.0, January 2004\n                     http://www.apache.org/licenses/\n\nTERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION\n\n1. Definitions.\n\n   "License" shall mean the terms and conditions for use, reproduction,\n   and distribution as defined by Sections 1 through 9 of this document.\n\n   "Licensor" shall mean the copyright owner or entity authorized by\n   the copyright owner that is granting the License.\n\n   "Legal Entity" shall mean the union of the acting entity and all\n   other entities that control, are controlled by, or are under common\n   control with that entity. For the purposes of this definition,\n   "control" means (i) the power, direct or indirect, to cause the\n   direction or management of such entity, whether by contract or\n   otherwise, or (ii) ownership of fifty percent (50%) or more of the\n   outstanding shares, or (iii) beneficial ownership of such entity.\n\n   "You" (or "Your") shall mean an individual or Legal Entity\n   exercising permissions granted by this License.\n\n   "Source" form shall mean the preferred form for making modifications,\n   including but not limited to software source code, documentation\n   source, and configuration files.\n\n   "Object" form shall mean any form resulting from mechanical\n   transformation or translation of a Source form, including but\n   not limited to compiled object code, generated documentation,\n   and conversions to other media types.\n\n   "Work" shall mean the work of authorship, whether in Source or\n   Object form, made available under the License, as indicated by a\n   copyright notice that is included in or attached to the work\n   (an example is provided in the Appendix below).\n\n   "Derivative Works" shall mean any work, whether in Source or Object\n   form, that is based on (or derived from) the Work and for which the\n   editorial revisions, annotations, elaborations, or other modifications\n   represent, as a whole, an original work of authorship. For the purposes\n   of this License, Derivative Works shall not include works that remain\n   separable from, or merely link (or bind by name) to the interfaces of,\n   the Work and Derivative Works thereof.\n\n   "Contribution" shall mean any work of authorship, including\n   the original version of the Work and any modifications or additions\n   to that Work or Derivative Works thereof, that is intentionally\n   submitted to Licensor for inclusion in the Work by the copyright owner\n   or by an individual or Legal Entity authorized to submit on behalf of\n   the copyright owner. For the purposes of this definition, "submitted"\n   means any form of electronic, verbal, or written communication sent\n   to the Licensor or its representatives, including but not limited to\n   communication on electronic mailing lists, source code control systems,\n   and issue tracking systems that are managed by, or on behalf of, the\n   Licensor for the purpose of discussing and improving the Work, but\n   excluding communication that is conspicuously marked or otherwise\n   designated in writing by the copyright owner as "Not a Contribution."\n\n   "Contributor" shall mean Licensor and any individual or Legal Entity\n   on behalf of whom a Contribution has been received by Licensor and\n   subsequently incorporated within the Work.\n\n2. Grant of Copyright License. Subject to the terms and conditions of\n   this License, each Contributor hereby grants to You a perpetual,\n   worldwide, non-exclusive, no-charge, royalty-free, irrevocable\n   copyright license to reproduce, prepare Derivative Works of,\n   publicly display, publicly perform, sublicense, and distribute the\n   Work and such Derivative Works in Source or Object form.\n\n3. Grant of Patent License. Subject to the terms and conditions of\n   this License, each Contributor hereby grants to You a perpetual,\n   worldwide, non-exclusive, no-charge, royalty-free, irrevocable\n   (except as stated in this section) patent license to make, have made,\n   use, offer to sell, sell, import, and otherwise transfer the Work,\n   where such license applies only to those patent claims licensable\n   by such Contributor that are necessarily infringed by their\n   Contribution(s) alone or by combination of their Contribution(s)\n   with the Work to which such Contribution(s) was submitted. If You\n   institute patent litigation against any entity (including a\n   cross-claim or counterclaim in a lawsuit) alleging that the Work\n   or a Contribution incorporated within the Work constitutes direct\n   or contributory patent infringement, then any patent licenses\n   granted to You under this License for that Work shall terminate\n   as of the date such litigation is filed.\n\n4. Redistribution. You may reproduce and distribute copies of the\n   Work or Derivative Works thereof in any medium, with or without\n   modifications, and in Source or Object form, provided that You\n   meet the following conditions:\n\n   (a) You must give any other recipients of the Work or\n       Derivative Works a copy of this License; and\n\n   (b) You must cause any modified files to carry prominent notices\n       stating that You changed the files; and\n\n   (c) You must retain, in the Source form of any Derivative Works\n       that You distribute, all copyright, patent, trademark, and\n       attribution notices from the Source form of the Work,\n       excluding those notices that do not pertain to any part of\n       the Derivative Works; and\n\n   (d) If the Work includes a "NOTICE" text file as part of its\n       distribution, then any Derivative Works that You distribute must\n       include a readable copy of the attribution notices contained\n       within such NOTICE file, excluding those notices that do not\n       pertain to any part of the Derivative Works, in at least one\n       of the following places: within a NOTICE text file distributed\n       as part of the Derivative Works; within the Source form or\n       documentation, if provided along with the Derivative Works; or,\n       within a display generated by the Derivative Works, if and\n       wherever such third-party notices normally appear. The contents\n       of the NOTICE file are for informational purposes only and\n       do not modify the License. You may add Your own attribution\n       notices within Derivative Works that You distribute, alongside\n       or as an addendum to the NOTICE text from the Work, provided\n       that such additional attribution notices cannot be construed\n       as modifying the License.\n\n   You may add Your own copyright statement to Your modifications and\n   may provide additional or different license terms and conditions\n   for use, reproduction, or distribution of Your modifications, or\n   for any such Derivative Works as a whole, provided Your use,\n   reproduction, and distribution of the Work otherwise complies with\n   the conditions stated in this License.\n\n5. Submission of Contributions. Unless You explicitly state otherwise,\n   any Contribution intentionally submitted for inclusion in the Work\n   by You to the Licensor shall be under the terms and conditions of\n   this License, without any additional terms or conditions.\n   Notwithstanding the above, nothing herein shall supersede or modify\n   the terms of any separate license agreement you may have executed\n   with Licensor regarding such Contributions.\n\n6. Trademarks. This License does not grant permission to use the trade\n   names, trademarks, service marks, or product names of the Licensor,\n   except as required for reasonable and customary use in describing the\n   origin of the Work and reproducing the content of the NOTICE file.\n\n7. Disclaimer of Warranty. Unless required by applicable law or\n   agreed to in writing, Licensor provides the Work (and each\n   Contributor provides its Contributions) on an "AS IS" BASIS,\n   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or\n   implied, including, without limitation, any warranties or conditions\n   of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A\n   PARTICULAR PURPOSE. You are solely responsible for determining the\n   appropriateness of using or redistributing the Work and assume any\n   risks associated with Your exercise of permissions under this License.\n\n8. Limitation of Liability. In no event and under no legal theory,\n   whether in tort (including negligence), contract, or otherwise,\n   unless required by applicable law (such as deliberate and grossly\n   negligent acts) or agreed to in writing, shall any Contributor be\n   liable to You for damages, including any direct, indirect, special,\n   incidental, or consequential damages of any character arising as a\n   result of this License or out of the use or inability to use the\n   Work (including but not limited to damages for loss of goodwill,\n   work stoppage, computer failure or malfunction, or any and all\n   other commercial damages or losses), even if such Contributor\n   has been advised of the possibility of such damages.\n\n9. Accepting Warranty or Additional Liability. While redistributing\n   the Work or Derivative Works thereof, You may choose to offer,\n   and charge a fee for, acceptance of support, warranty, indemnity,\n   or other liability obligations and/or rights consistent with this\n   License. However, in accepting such obligations, You may act only\n   on Your own behalf and on Your sole responsibility, not on behalf\n   of any other Contributor, and only if You agree to indemnify,\n   defend, and hold each Contributor harmless for any liability\n   incurred by, or claims asserted against, such Contributor by reason\n   of your accepting any such warranty or additional liability.\n\nEND OF TERMS AND CONDITIONS\n'}
APACHE = LEGAL_FIXTURES['Apache-2.0.txt']

def archive(entries, name='dep', version='1.0.0', kinds=None):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode='w:gz') as target:
        for path, raw in entries:
            member = tarfile.TarInfo(path if path.startswith('/') else name + '-' + version + '/' + path)
            member.size = len(raw)
            if kinds and path in kinds:
                member.type = kinds[path]
                member.linkname = '../../outside'
                member.size = 0
                target.addfile(member)
            else: target.addfile(member, io.BytesIO(raw))
    return stream.getvalue()

class Fixture:
    def __init__(self, base):
        self.base = base
        self.project = base / 'project'; self.project.mkdir()
        self.manifest = self.project / 'Cargo.toml'
        self.manifest.write_text('[package]\nname="fixture"\nversion="1.0.0"\n')
        self.cargo_home = base / 'cache'; self.cargo_home.mkdir()
        self.supplement = base / 'supplement/materials.json'; self.supplement.parent.mkdir()
        self.supplement.write_bytes(module.json_bytes({'schemaVersion': 1, 'edition': 'fixture', 'components': []}))
        self.sysroot = base / 'sysroot'
        self.library_notice = self.sysroot / 'share/doc/rust/COPYRIGHT-library.html'
        self.library_notice.parent.mkdir(parents=True)
        self.library_notice.write_text('<html><title>Copyright notices for The Rust Standard Library</title><pre>' + MIT + APACHE + '</pre></html>')
        self.tools = base / 'tools'; self.tools.mkdir()
        self.cargo = self.tools / 'cargo'; self.cargo.write_bytes(b'fake-cargo-tool')
        self.rustc = self.tools / 'rustc'; self.rustc.write_bytes(b'fake-rustc-tool')
        self.options = SimpleNamespace(edition='fixture', package='fixture', target='aarch64-apple-darwin', host='aarch64-apple-darwin', profile='release', feature_mode='default', features=[], manifest=self.manifest, lock=self.project / 'Cargo.lock', cargo_home=self.cargo_home, supplement=self.supplement, cargo=self.cargo, rustc=self.rustc)
        self.packages = {}; self.target_names = []; self.host_names = []
        self.normal_names = []; self.build_names = []
        self.calls = []; self.extra_tree = None; self.override_host = None; self.override_release = None
        self.root = {'id': 'path+file://' + str(self.project) + '#fixture@1.0.0', 'name': 'fixture', 'version': '1.0.0', 'source': None, 'manifest_path': str(self.manifest), 'license': None, 'targets': [{'kind': ['bin']}], 'features': {}}
        self.add_package('dep')
        self.target_names = ['dep']; self.normal_names = ['dep']; self.build_names = ['dep']
        self.write_lock()
    def add_package(self, name, license='MIT', extra=None, license_text=MIT, proc_macro=False):
        version = '1.0.0'
        cargo = f'[package]\nname="{name}"\nversion="{version}"\nlicense="{license}"\nauthors=["Author Metadata Only"]\n'.encode()
        entries = [('Cargo.toml', cargo), ('src/lib.rs', b'pub fn fixture() {}\n'), ('.cargo_vcs_info.json', module.json_bytes({'git': {'sha1': 'a' * 40}, 'path_in_vcs': ''}))]
        if license_text is not None: entries += [('LICENSE', license_text.encode())]
        entries += extra or []
        raw = archive(entries, name, version)
        source = self.cargo_home / 'registry/src/index.fixture' / (name + '-' + version); source.mkdir(parents=True)
        for path, data in entries:
            f = source / path; f.parent.mkdir(parents=True, exist_ok=True); f.write_bytes(data)
        (source / '.cargo-ok').write_text('unauthenticated cache marker')
        cached = self.cargo_home / 'registry/cache/index.fixture' / (name + '-' + version + '.crate'); cached.parent.mkdir(parents=True, exist_ok=True); cached.write_bytes(raw)
        self.packages[name] = {'metadata': {'id': module.REGISTRY + '#' + name + '@' + version, 'name': name, 'version': version, 'source': module.REGISTRY, 'manifest_path': str(source / 'Cargo.toml'), 'license': license, 'targets': [{'kind': ['proc-macro' if proc_macro else 'lib']}], 'features': {}}, 'checksum': module.sha(raw), 'archive': cached, 'source': source}
    def write_lock(self):
        text = 'version = 4\n'
        for name, p in self.packages.items():
            text += f'\n[[package]]\nname = "{name}"\nversion = "1.0.0"\nsource = "{module.REGISTRY}"\nchecksum = "{p["checksum"]}"\n'
        self.options.lock.write_text(text)
    def set_supplement(self, name='dep', raw=MIT, kind='canonical-license-terms', gaps=None, review=None, target=None, checksum=None, commit=None):
        p = self.supplement.parent / 'fixed/LICENSE.txt'; p.parent.mkdir(exist_ok=True); p.write_text(raw)
        record = {'name': name, 'version': '1.0.0', 'registryChecksum': checksum or self.packages[name]['checksum'], 'licenseDeclaration': self.packages[name]['metadata']['license'], 'selectedLicense': 'MIT', 'upstream': {'commit': commit or 'a' * 40, 'repository': 'https://example.invalid/fixture'}, 'complete': not gaps, 'gaps': gaps or [], 'additionalReview': review or [], 'applicableTargets': [{'edition': 'fixture', 'target': target or self.options.target}], 'files': [{'path': 'fixed/LICENSE.txt', 'url': 'https://example.invalid/fixed', 'sha256': module.sha(raw.encode()), 'bytes': len(raw.encode()), 'kind': kind, 'licenseId': 'MIT', 'containsComponentCopyright': kind != 'canonical-license-terms'}]}
        self.supplement.write_bytes(module.json_bytes({'schemaVersion': 1, 'edition': 'fixture', 'components': [record]}))
    def runner(self, args, cwd, env):
        self.calls.append(args)
        if args[0] == str(self.cargo) and args[1] == '-Vv': return ('cargo 1.98.1\nhost: ' + self.options.host + '\n').encode(), b''
        if args[0] == str(self.rustc) and args[1] == '-Vv':
            return ('rustc 1.98.1\nhost: ' + (self.override_host or self.options.host) + '\nrelease: ' + (self.override_release or '1.98.1') + '\ncommit-hash: ' + 'a' * 40 + '\n').encode(), b''
        if args[0] == str(self.rustc) and args[1:] == ['--print', 'sysroot']: return str(self.sysroot).encode(), b''
        if args[1] == 'metadata':
            names = self.target_names if args[args.index('--filter-platform') + 1] == self.options.target else self.host_names
            metadata = {'version': 1, 'workspace_root': str(self.project), 'packages': [self.root] + [self.packages[n]['metadata'] for n in names]}
            return module.json_bytes(metadata), b''
        if args[1] == 'tree':
            names = self.normal_names if args[args.index('--edges') + 1] == 'normal' else self.build_names
            lines = ['fixture v1.0.0 (' + str(self.project) + ')|||']
            for n in names: lines.append(n + ' v1.0.0' + (' (proc-macro)' if self.packages[n]['metadata']['targets'][0]['kind'] == ['proc-macro'] else '') + '|' + self.packages[n]['metadata']['license'] + '|https://example.invalid/fixture|')
            if self.extra_tree: lines += self.extra_tree
            return ('\n'.join(lines) + '\n').encode(), b''
        raise AssertionError(args)
    def collect(self): return module.build_candidate(self.options, self.runner)
    def published(self):
        manifest, files = self.collect(); output = self.base / 'candidate'; module.publish(output, files, self.options); return manifest, output

class CollectorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.base = Path(self.temp.name); self.fixture = Fixture(self.base)
    def tearDown(self): self.temp.cleanup()
    def test_deterministic_full_generation_and_read_only_strict_verify(self):
        m, out = self.fixture.published(); before = {p.relative_to(out).as_posix(): p.read_bytes() for p in out.rglob('*') if p.is_file()}
        self.assertEqual(m['summary']['components'], 1); self.assertEqual(m['summary']['strictCandidateMaterialGate'], 'pass')
        module.verify(self.fixture.options, out, strict=True, runner=self.fixture.runner)
        after = {p.relative_to(out).as_posix(): p.read_bytes() for p in out.rglob('*') if p.is_file()}; self.assertEqual(before, after)
        self.assertEqual(self.fixture.collect()[0], m)
    def test_target_host_mapping_excludes_metadata_extras_and_marks_roles(self):
        f = self.fixture; f.options.target = 'x86_64-pc-windows-msvc'; f.add_package('host-only', proc_macro=True); f.add_package('unused'); f.write_lock(); f.target_names += ['unused']; f.host_names = ['host-only']; f.build_names += ['host-only']
        m, _ = f.collect(); self.assertEqual([c['name'] for c in m['components']], ['dep', 'host-only']); self.assertEqual(m['summary']['buildExclusiveCandidates'], 1); self.assertEqual(m['summary']['procMacroInputs'], 1)
        _, out = f.published(); module.verify(f.options, out, runner=f.runner)
        with self.assertRaises(module.StrictError): module.verify(f.options, out, strict=True, runner=f.runner)
    def test_missing_target_and_host_identity_fails(self):
        self.fixture.target_names = []
        with self.assertRaisesRegex(module.Error, 'Missing/ambiguous'): self.fixture.collect()
    def test_ambiguous_metadata_source_fails(self):
        f = self.fixture; original = f.runner
        def runner(args, cwd, env):
            raw, err = original(args, cwd, env)
            if len(args) > 1 and args[1] == 'metadata':
                data = json.loads(raw); p = copy.deepcopy(data['packages'][1]); p['id'] += '-other'; p['source'] = 'git+https://example.invalid/fixture#abc'; data['packages'].append(p); raw = module.json_bytes(data)
            return raw, err
        with self.assertRaisesRegex(module.Error, 'Missing/ambiguous'): module.build_candidate(f.options, runner)
    def test_duplicate_tree_rows_do_not_duplicate_component(self):
        self.fixture.extra_tree = ['dep v1.0.0|MIT|https://example.invalid/fixture| (*)']
        m, _ = self.fixture.collect(); self.assertEqual(len(m['components']), 1)
    def test_unparseable_tree_is_not_silently_skipped(self):
        self.fixture.extra_tree = ['garbage unknown row']
        with self.assertRaisesRegex(module.Error, 'Unparseable'): self.fixture.collect()
    def test_normal_tree_must_be_subset(self):
        f = self.fixture; f.add_package('normal-only'); f.write_lock(); f.target_names += ['normal-only']; f.normal_names += ['normal-only']
        with self.assertRaisesRegex(module.Error, 'Inconsistent tree'): f.collect()
    def test_explicit_host_mismatch_fails(self):
        self.fixture.override_host = 'x86_64-pc-windows-msvc'
        with self.assertRaisesRegex(module.Error, 'Explicit host'): self.fixture.collect()
    def test_explicit_workspace_lock_cannot_be_substituted(self):
        f = self.fixture; lock = self.base / 'other.lock'; lock.write_bytes(f.options.lock.read_bytes()); f.options.lock = lock
        with self.assertRaisesRegex(module.Error, 'workspace lock'): f.collect()
    def test_package_checksums_and_cache_markers_are_not_trusted(self):
        f = self.fixture; (f.packages['dep']['source'] / '.cargo-checksum.json').write_text('{}'); f.packages['dep']['archive'].write_bytes(b'bad data')
        with self.assertRaisesRegex(module.Error, 'checksum mismatch'): f.collect()
    def test_missing_archive_fails_without_tree_fallback(self):
        self.fixture.packages['dep']['archive'].unlink()
        with self.assertRaisesRegex(module.Error, 'Expected regular input'): self.fixture.collect()
    def test_registry_source_tamper_fails(self):
        f = self.fixture; (f.packages['dep']['source'] / 'src/lib.rs').write_text('changed but .cargo-ok remains')
        with self.assertRaisesRegex(module.Error, 'source bytes differ'): f.collect()
    def test_extra_registry_source_file_fails(self):
        (self.fixture.packages['dep']['source'] / 'new.rs').write_text('extra')
        with self.assertRaisesRegex(module.Error, 'source file set drift'): self.fixture.collect()
    def test_nested_notices_are_retained_code_name_is_not_license(self):
        f = self.fixture; f.add_package('nested', extra=[('third_party/vendor/NOTICE', b'Existing native attribution\n'), ('COPYING.rs', b'pub fn copying() {}\n')]); f.write_lock(); f.target_names += ['nested']; f.normal_names += ['nested']; f.build_names += ['nested']
        m, _ = f.collect(); c = next(c for c in m['components'] if c['name'] == 'nested'); paths = [x['sourcePath'] for x in c['materials']]
        self.assertIn('third_party/vendor/NOTICE', paths); self.assertNotIn('COPYING.rs', paths)
    def test_license_file_nonstandard_filename_is_collected(self):
        f = self.fixture; p = f.packages['dep']; entries = [('Cargo.toml', b'[package]\nname="dep"\nversion="1.0.0"\nlicense="MIT"\nlicense-file="TERMS.txt"\n'), ('src/lib.rs', b'fn test() {}'), ('TERMS.txt', MIT.encode())]; raw = archive(entries); p['archive'].write_bytes(raw); p['checksum'] = module.sha(raw)
        for existing in p['source'].rglob('*'):
            if existing.is_file(): existing.unlink()
        for path, data in entries:
            q = p['source'] / path; q.parent.mkdir(parents=True, exist_ok=True); q.write_bytes(data)
        f.write_lock(); m, _ = f.collect(); self.assertIn('TERMS.txt', [t['sourcePath'] for t in m['components'][0]['materials']])
    def test_canonical_with_declared_gap_never_closes_component(self):
        f = self.fixture; f.set_supplement(raw=MIT.replace('2026 Fixture Authors', '<year> <copyright holders>'), gaps=[{'kind': 'actual-copyright-notice-not-published'}]); m, out = f.published(); c = m['components'][0]
        self.assertTrue(c['gaps']); self.assertFalse(c['licenseMaterialsReady']); self.assertEqual(c['canonicalTerms'], ['MIT'])
        module.verify(f.options, out, runner=f.runner)
        with self.assertRaises(module.StrictError): module.verify(f.options, out, strict=True, runner=f.runner)
    def test_authors_and_template_placeholders_are_not_copyright(self):
        self.assertEqual(module.actual_copyright_lines('Copyright (c) <year> <copyright holders>\nAUTHORS: Alice'), [])
        self.assertEqual(module.actual_copyright_lines('Copyright (c) 2026 Actual Fixture Owner\n')[0][0], 1)
    def test_supplement_revision_checksum_and_target_mismatch_fail(self):
        f = self.fixture
        for kw, text in [({'checksum': 'b' * 64}, 'locked component mismatch'), ({'commit': 'b' * 40}, 'fixed revision differs'), ({'target': 'other-target'}, 'edition/target mismatch')]:
            with self.subTest(kw=kw):
                f.set_supplement(**kw)
                with self.assertRaisesRegex(module.Error, text): f.collect()
    def test_supplement_extra_file_and_tamper_fail(self):
        f = self.fixture; f.set_supplement(); (f.supplement.parent / 'extra.txt').write_text('untracked')
        with self.assertRaisesRegex(module.Error, 'extra files'): f.collect()
        (f.supplement.parent / 'extra.txt').unlink(); (f.supplement.parent / 'fixed/LICENSE.txt').write_text('tamper')
        with self.assertRaisesRegex(module.Error, 'hash/size mismatch'): f.collect()
    def test_additional_sdk_review_survives_and_blocks_strict(self):
        f = self.fixture; f.set_supplement(kind='upstream-license-terms', review=[{'kind': 'apple-sdk-derived-bindings'}]); _, out = f.published()
        with self.assertRaises(module.StrictError): module.verify(f.options, out, strict=True, runner=f.runner)
    def test_input_feature_mode_profile_and_compiler_drift_fail_verify(self):
        f = self.fixture; _, out = f.published()
        for attribute, value in [('features', ['new']), ('feature_mode', 'no-default'), ('profile', 'dev')]:
            before = getattr(f.options, attribute); setattr(f.options, attribute, value)
            with self.subTest(attribute=attribute), self.assertRaisesRegex(module.Error, 'inputs differ'): module.verify(f.options, out, runner=f.runner)
            setattr(f.options, attribute, before)
        f.override_release = '1.99.0'
        with self.assertRaisesRegex(module.Error, 'inputs differ'): module.verify(f.options, out, runner=f.runner)
    def test_cargo_config_and_tool_bytes_drift_fail_verify(self):
        f = self.fixture; _, out = f.published(); cfg = f.project / '.cargo/config.toml'; cfg.parent.mkdir(); cfg.write_text('[build]\ntarget="other"')
        with self.assertRaisesRegex(module.Error, 'inputs differ'): module.verify(f.options, out, runner=f.runner)
        cfg.unlink(); f.cargo.write_bytes(b'new fake cargo binary')
        with self.assertRaisesRegex(module.Error, 'inputs differ'): module.verify(f.options, out, runner=f.runner)
    def test_new_lock_snapshot_drift_fails_verify(self):
        f = self.fixture; _, out = f.published(); f.options.lock.write_text(f.options.lock.read_text() + '\n# drift\n')
        with self.assertRaisesRegex(module.Error, 'inputs differ'): module.verify(f.options, out, runner=f.runner)
    def test_missing_standard_library_notice_not_replaced_by_compiler_notice(self):
        f = self.fixture; f.library_notice.rename(f.library_notice.with_name('COPYRIGHT.html')); m, out = f.published(); self.assertTrue(m['stdlib']['gaps'])
        with self.assertRaises(module.StrictError): module.verify(f.options, out, strict=True, runner=f.runner)
    def test_stdlib_notice_byte_drift_fails_verify(self):
        f = self.fixture; _, out = f.published(); f.library_notice.write_text(f.library_notice.read_text().replace('Fixture Authors', 'Changed Authors'))
        with self.assertRaisesRegex(module.Error, 'inputs differ'): module.verify(f.options, out, runner=f.runner)
    def test_output_extra_file_and_empty_directory_fail(self):
        _, out = self.fixture.published(); (out / 'extra.txt').write_text('extra')
        with self.assertRaisesRegex(module.Error, 'extra files'): module.verify_directory(out)
        (out / 'extra.txt').unlink(); (out / 'extra-dir').mkdir()
        with self.assertRaisesRegex(module.Error, 'extra files'): module.verify_directory(out)
    def test_output_symlink_fails(self):
        _, out = self.fixture.published(); (out / 'link').symlink_to(self.fixture.manifest)
        with self.assertRaises(module.Error): module.verify_directory(out)
    def test_verify_never_repairs_tampered_output(self):
        f = self.fixture; _, out = f.published(); notices = out / 'THIRD-PARTY-NOTICES.txt'; notices.write_text('tampered')
        with self.assertRaisesRegex(module.Error, 'Output hash'): module.verify(f.options, out, runner=f.runner)
        self.assertEqual(notices.read_text(), 'tampered')
    def test_existing_unowned_directory_is_never_replaced(self):
        f = self.fixture; _, files = f.collect(); out = self.base / 'unowned'; out.mkdir(); (out / 'important').write_text('keep')
        with self.assertRaises(module.Error): module.publish(out, files, f.options)
        self.assertEqual((out / 'important').read_text(), 'keep')
    def test_failed_install_rename_restores_old_complete_directory(self):
        f = self.fixture; m, out = f.published(); _, files = f.collect(); real_replace = module.os.replace; calls = []
        def replace(a, b):
            calls.append((str(a), str(b)))
            if '.stage-' in str(a): raise OSError('injected install failure')
            return real_replace(a, b)
        with patch.object(module.os, 'replace', replace), self.assertRaisesRegex(OSError, 'injected'): module.publish(out, files, f.options)
        self.assertEqual(module.verify_directory(out), m); self.assertEqual(len(calls), 3); self.assertFalse(list(out.parent.glob('.candidate.stage-*'))); self.assertFalse(list(out.parent.glob('.candidate.backup-*')))
    def test_staging_write_failure_keeps_previous_directory(self):
        f = self.fixture; m, out = f.published(); _, files = f.collect()
        with patch.object(module.os, 'fsync', side_effect=OSError('injected write failure')), self.assertRaises(OSError): module.publish(out, files, f.options)
        self.assertEqual(module.verify_directory(out), m)
    def test_concurrent_writer_and_input_overlap_fail(self):
        f = self.fixture; _, files = f.collect(); out = self.base / 'candidate'
        with module.output_lock(out), self.assertRaisesRegex(module.Error, 'already locked'): module.publish(out, files, f.options)
        with self.assertRaisesRegex(module.Error, 'overlaps'): module.publish(f.project / 'candidate', files, f.options)
    def test_tree_commands_are_explicit_locked_offline_and_features_recorded(self):
        f = self.fixture; f.options.feature_mode = 'no-default'; f.options.features = ['foo']; f.collect()
        queries = [args for args in f.calls if len(args) > 1 and args[1] in ('tree', 'metadata')]
        for args in queries: self.assertIn('--locked', args); self.assertIn('--offline', args); self.assertIn('--no-default-features', args); self.assertIn('--features', args)
    def test_input_change_during_query_aborts_generation(self):
        f = self.fixture; real = f.runner; changed = False
        def runner(args, cwd, env):
            nonlocal changed
            result = real(args, cwd, env)
            if not changed and args[1] == 'tree': f.options.lock.write_text(f.options.lock.read_text() + '\n# concurrently changed\n'); changed = True
            return result
        with self.assertRaisesRegex(module.Error, 'input drift'): module.build_candidate(f.options, runner)
    def test_archive_source_and_supplement_changes_after_collect_reject_publish(self):
        for kind in ['archive', 'source-file', 'source-extra-file', 'source-extra-directory', 'supplement-extra-file']:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as tmp:
                f = Fixture(Path(tmp)); _, files = f.collect(); out = f.base / 'candidate'
                if kind == 'archive': f.packages['dep']['archive'].write_bytes(b'changed archive')
                elif kind == 'source-file': (f.packages['dep']['source'] / 'src/lib.rs').write_text('changed source')
                elif kind == 'source-extra-file': (f.packages['dep']['source'] / 'extra').write_text('extra')
                elif kind == 'source-extra-directory': (f.packages['dep']['source'] / 'empty-extra').mkdir()
                else: (f.supplement.parent / 'extra').write_text('extra')
                with self.assertRaisesRegex(module.Error, 'changed during collection'): module.publish(out, files, f.options)
                self.assertFalse(out.exists()); self.assertFalse(list(out.parent.glob('.candidate.stage-*')))
    def test_source_extra_during_source_verification_cannot_enter_snapshot(self):
        f = self.fixture; original = module.check_source_tree
        def tamper(base, contents):
            result = original(base, contents); (base / 'new-source').write_text('added after check'); return result
        with patch.object(module, 'check_source_tree', tamper), self.assertRaisesRegex(module.Error, 'Input directory changed'): f.collect()
    def test_exact_package_cache_wait_diagnostic_is_logged_and_not_stale(self):
        f = self.fixture; original = f.runner
        def waiting(args, cwd, env):
            raw, err = original(args, cwd, env)
            return raw, b'    Blocking waiting for file lock on package cache\n' if args[1] == 'tree' else err
        with patch.object(module.sys, 'stderr', io.StringIO()) as log:
            m, files = module.build_candidate(f.options, waiting)
        self.assertIn('Blocking waiting', log.getvalue())
        self.assertEqual(m, f.collect()[0])
        self.assertNotIn('inputs/tree-normal.stderr.txt', files)
        def warning(args, cwd, env):
            raw, err = original(args, cwd, env)
            return raw, b'warning: actual unexpected condition\n' if args[1] == 'tree' else err
        other, data = module.build_candidate(f.options, warning)
        self.assertNotEqual(m, other); self.assertIn('inputs/tree-normal.stderr.txt', data)
    def test_target_profile_environment_fingerprints_reject_stale_without_plain_values(self):
        f = self.fixture; _, out = f.published()
        key = 'CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS'
        with patch.dict(module.os.environ, {key: 'fixture-private-build-flag'}):
            m, _ = f.collect(); self.assertEqual(m['context']['environment'][key], module.sha(b'fixture-private-build-flag'))
            self.assertNotIn('fixture-private-build-flag', module.json_bytes(m).decode())
            with self.assertRaisesRegex(module.Error, 'inputs differ'): module.verify(f.options, out, runner=f.runner)
        original = f.runner
        def mutate(args, cwd, env):
            result = original(args, cwd, env)
            if args[1] == 'tree': module.os.environ['CARGO_PROFILE_RELEASE_LTO'] = 'true'
            return result
        with patch.dict(module.os.environ, {}, clear=False), self.assertRaisesRegex(module.Error, 'environment changed'): module.build_candidate(f.options, mutate)

class ParsingTests(unittest.TestCase):
    def test_and_or_slash_and_unknown_exception(self):
        self.assertEqual(module.Expression('MIT/Apache-2.0').satisfy({'MIT'}), ['MIT'])
        self.assertIsNone(module.Expression('MIT AND Apache-2.0').satisfy({'MIT'}))
        self.assertEqual(module.Expression('(MIT OR Apache-2.0) AND Unicode-3.0').satisfy({'MIT', 'Unicode-3.0'}), ['MIT', 'Unicode-3.0'])
        self.assertEqual(module.Expression('Apache-2.0 WITH LLVM-exception OR MIT').satisfy({'MIT'}), ['MIT'])
        with self.assertRaises(module.Error): module.Expression('MIT AND')
    def test_declaration_or_truncated_license_is_not_full_terms(self):
        self.assertEqual(module.full_term_ids('This library is licensed under MIT.'), set())
        self.assertEqual(module.full_term_ids(MIT[:300]), set())
        self.assertEqual(module.full_term_ids(APACHE_MARKERS), set())
    def test_reviewed_full_unicode_cc0_cdla_and_apache_texts(self):
        rows = LEGAL_FIXTURE_PROVENANCE
        for row in rows:
            with self.subTest(fixture=row['fixture']):
                raw = LEGAL_FIXTURES[row['fixture']].encode(); self.assertEqual(module.sha(raw), row['sha256'])
                identifier = row['fixture'].removesuffix('.txt'); text = raw.decode()
                self.assertIn(identifier, module.full_term_ids(text))
                self.assertNotIn(identifier, module.full_term_ids(text[:len(text)//2]))
    def test_archive_rejects_traversal_absolute_duplicate_link_and_special_members(self):
        cargo = ('Cargo.toml', b'[package]\nname="dep"\nversion="1.0.0"')
        cases = [[cargo, ('../../outside', b'x')], [cargo, ('/absolute', b'x')], [cargo, cargo]]
        for entries in cases:
            with self.subTest(entries=entries), self.assertRaises(module.Error): module.archive_contents(archive(entries), 'dep', '1.0.0')
        for kind in [tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE, tarfile.FIFOTYPE]:
            with self.subTest(kind=kind), self.assertRaises(module.Error): module.archive_contents(archive([cargo, ('link', b'')], kinds={'link': kind}), 'dep', '1.0.0')
    def test_archive_wrong_root_limits_and_truncation(self):
        raw = archive([('Cargo.toml', b'valid fixture')])
        with self.assertRaises(module.Error): module.archive_contents(raw, 'wrong', '1.0.0')
        with patch.object(module, 'MAX_ARCHIVE', 1), self.assertRaises(module.Error): module.archive_contents(raw, 'dep', '1.0.0')
        with patch.object(module, 'MAX_EXPANDED', 1), self.assertRaises(module.Error): module.archive_contents(raw, 'dep', '1.0.0')
        with self.assertRaises(module.Error): module.archive_contents(raw[:40], 'dep', '1.0.0')
    def test_unsafe_paths_and_invalid_license_encoding(self):
        for path in ['../escape', '/absolute', 'C:/file', 'a\\b', './file', 'a/../b']:
            with self.subTest(path=path), self.assertRaises(module.Error): module.safe_relative(path)
        with self.assertRaises(module.Error): module.add_material({}, 'dep', '1.0.0', 'LICENSE', b'\xff', 'authenticated-crate-text')
    def test_bounded_read_rejects_file_growth_after_advisory_stat(self):
        with tempfile.TemporaryDirectory() as tmp:
            real = Path(tmp) / 'growing'; real.write_bytes(b'123456789')
            growth = SimpleNamespace(is_symlink=lambda: False, is_file=lambda: True,
                                     stat=lambda: SimpleNamespace(st_size=1), open=real.open)
            with self.assertRaisesRegex(module.Error, 'grew beyond'):
                module.regular_bytes(growth, 4)
    def test_symbol_and_spdx_copyright_retained_without_template_holders(self):
        lines = module.actual_copyright_lines('// © 2026 Fixture Owner\n# SPDX-FileCopyrightText: 2024 Fixture Developer\nSPDX-FileCopyrightText: <year> <copyright holders>\nSPDX-FileCopyrightText: Actual Fixture Rights Holder\n')
        self.assertEqual([n for n, _ in lines], [1, 2, 4])
        self.assertEqual(module.actual_copyright_lines('SPDX-FileCopyrightText: NONE\nPackageCopyrightText: NOASSERTION\nCopyright (c) <YEAR> Fixture Owner\n'), [])
    def test_source_header_excerpt_stops_before_api_docs(self):
        raw = b'// Copyright 2026 Fixture Authors\n// Licensed under MIT.\n\n/// Example code to exclude\nfn fixture() {}\n'
        notes = module.source_notices({'src/lib.rs': raw}); self.assertEqual(notes[0]['lastLine'], 3); self.assertNotIn('Example', notes[0]['text']); self.assertEqual(notes[0]['sourceSha256'], module.sha(raw))

if __name__ == '__main__': unittest.main(verbosity=2)

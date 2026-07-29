# Changelog

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.12.0...kobako-transport-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **runtime:** mark the sets that grow, and say why the closed ones do not
* **transport:** let every envelope spell its success arm the same way
* **transport:** name the core envelope's types after what they carry
* **wire:** carry a Reply's fault arm on the envelope
* **transport:** split the invocation module along the line its doc draws

### Features

* **runtime:** mark the sets that grow, and say why the closed ones do not ([1be2449](https://github.com/elct9620/kobako/commit/1be24492961e2c2a3a317f784cc2f20ae584dcbf))
* **transport:** add kobako-transport, one implementation of the envelope ([23f2401](https://github.com/elct9620/kobako/commit/23f240111a4c59e8f521bec911f9cf1cd7287744))
* **transport:** give the ABI's fixed values one definition each ([94754ec](https://github.com/elct9620/kobako/commit/94754ecb538286077b6b58d90504b458ac0e4a2c))
* **wire:** carry a Reply's fault arm on the envelope ([0bce850](https://github.com/elct9620/kobako/commit/0bce850b3416696e92ae6ee12c353c4c21c8e583))


### Bug Fixes

* **wire:** keep the ABI at 3, which no release has shipped ([2b301b7](https://github.com/elct9620/kobako/commit/2b301b777384c0edf79426805cfb1cc08688aa95))


### Code Refactoring

* **transport:** let every envelope spell its success arm the same way ([df595e0](https://github.com/elct9620/kobako/commit/df595e08097af9e3e0a0c5ed52a4a6f854eb21dc))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **transport:** split the invocation module along the line its doc draws ([4d3a2d3](https://github.com/elct9620/kobako/commit/4d3a2d3060167d71a807e203688186740fd1485b))

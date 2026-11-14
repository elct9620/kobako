# Changelog

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.20.0...kobako-runtime-v0.21.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* **sdk:** `kobako::Options` and `kobako_wasmtime::Config` read and set every cap through `limits`: write `options.limits.timeout = …` where `options.timeout = …` was written, and likewise for `setup_timeout`, `memory_limit`, `stdout_limit`, `stderr_limit`, `open_file_limit`, `open_connection_limit`, and `connection_limit`.
* **transport:** kobako_transport::abi::MAX_DISPATCH_PAYLOAD is renamed to MAX_MESSAGE_LEN; its value is unchanged.
* **runtime:** `memory_limit` now bounds the guest's `memory.grow` and what the host holds for the run together, where a Filesystem written in Ruby had its directory listings counted against a limit of their own. A run that grew close to `memory_limit` and listed a large directory may now raise `Errno::ENOMEM` in the guest or `Kobako::MemoryLimitError`, and `Usage#memory_peak` includes the listings. Raise `memory_limit` by what those listings hold to keep the earlier headroom.
* **runtime:** `kobako_runtime::FilesystemError`, `kobako_runtime::vfs::FilesystemError`, and `kobako_runtime::filesystem::FilesystemError` are now `kobako_runtime::Errno`, also reachable as `kobako::Errno`. Rename the type in a Filesystem or security module; its variants are unchanged.
* **runtime:** FileOperations::poll takes a second parameter, Option<&mut PollTable>; write fn poll(&self, mode: FMode, _table: Option<&mut PollTable>) in place of fn poll(&self, mode: FMode). File::poll takes the table too; pass None for a poll that will not sleep.
* **runtime:** kobako_runtime::mount::Policy moves to kobako_runtime::grant::Policy<R>. Implement Policy<MountRequest<'_>> with allows(&self, request: &MountRequest<'_>) in place of allows(&self, path: &str, access: Access), reading request.path and request.access; a mount_with_policy policy is now Arc<dyn for<'a> Policy<MountRequest<'a>>>, which a closure also fills.

### Features

* bound a guest's connections per Sandbox, per run, and per process ([7a4f869](https://github.com/elct9620/kobako/commit/7a4f8692cbc788a23c582b90112cd0275739d496))
* **filesystem:** let a Host App mark a Filesystem only the guest changes ([b7e0c81](https://github.com/elct9620/kobako/commit/b7e0c81b7338bd16243a9157de5c8194227b95c8))
* **runtime:** add the guest's TCP socket as a socket file over a Protocol Family ([d887f60](https://github.com/elct9620/kobako/commit/d887f603a7ac2587f6ffbc20f099c130d5ee43ef))
* **runtime:** answer the guest's network from a table a test writes ([b3aba65](https://github.com/elct9620/kobako/commit/b3aba650760b8cff3113341500fa414b17f4dd76))
* **runtime:** cap a socket's worker threads per Sandbox and per process ([d9b81fc](https://github.com/elct9620/kobako/commit/d9b81fcf010cb695efdc2370a7b12148d53a3bcd))
* **runtime:** dial each address a name may lead to in turn ([cea16d3](https://github.com/elct9620/kobako/commit/cea16d3e4f92251ebd97baf07cd113c7d926d271))
* **runtime:** end a guest's HTTPS on the host, trusting the OS's own store ([5809be2](https://github.com/elct9620/kobako/commit/5809be20fb1127f29811e97062e68cfb11972b82))
* **runtime:** give the guest named network entries as a Grant ([85cfc10](https://github.com/elct9620/kobako/commit/85cfc10b029c2a94adc45d1809fbbd7c651d94f9))
* **runtime:** hold a request's body against the Invocation's memory ([9125b12](https://github.com/elct9620/kobako/commit/9125b12979117e8086194c39efe90cc2ebd04056))
* **runtime:** hold a socket's hop limit, and a UDP socket's buffer sizes ([8cf7385](https://github.com/elct9620/kobako/commit/8cf7385474c9ef569a4fee7d025cc5887fe8bb8a))
* **runtime:** hold a TCP socket's keepalive probes and buffer sizes ([a435964](https://github.com/elct9620/kobako/commit/a435964d9d61fcfec7435061b230367008a25fc9))
* **runtime:** hold a TCP socket's keepalive setting for its host socket ([4562f51](https://github.com/elct9620/kobako/commit/4562f510685a92ae02050674968679e15171b637))
* **runtime:** judge a guest's HTTP request by its endpoint's rules and the request policies ([69de0da](https://github.com/elct9620/kobako/commit/69de0dac95d0dfd2ce8f76c8dc39fdd1f92235ef))
* **runtime:** judge a literal address against the endpoints and kobako's fixed sets ([b5351f8](https://github.com/elct9620/kobako/commit/b5351f85ec9177af847387120c3c52f728ed99b5))
* **runtime:** judge each connect through the socket hooks and each entry's policies ([82048b4](https://github.com/elct9620/kobako/commit/82048b4b43bca5fb0db6f1650561b0499aa83353))
* **runtime:** let a file's poll register on the poller's wait queue ([05fd5a6](https://github.com/elct9620/kobako/commit/05fd5a626a5ebd21f037e1bbacbf61350a5f5d89))
* **runtime:** refuse a DNS endpoint without an exact name ([2dcd401](https://github.com/elct9620/kobako/commit/2dcd401f680399cbf1d8cf4b8036e36243851af7))
* **runtime:** resolve a name to a virtual address the guest connects through ([0dde700](https://github.com/elct9620/kobako/commit/0dde7000c2282d94c90151e8db6b2c9e2c45c837))
* **runtime:** send a guest's HTTP request through an outbound seam dialing only kobako's connector ([43c49fe](https://github.com/elct9620/kobako/commit/43c49fec9b7f7947cb8ed29daf20f3e3d1db8ca1))
* **runtime:** stream datagrams with one peer through a UDP socket file ([f72fd2e](https://github.com/elct9620/kobako/commit/f72fd2e80c8a85acdd66a07b394b723124a5c6ed))
* **runtime:** swap an entry's credential into a request bound to its own endpoints ([4b64ccb](https://github.com/elct9620/kobako/commit/4b64ccb61bcae18907bf424d2539052e9588875e))
* **runtime:** wait on wait queues that a producer or an interrupt wakes ([8b15e44](https://github.com/elct9620/kobako/commit/8b15e4484587d3aed5cdf3a88386ffdb99974276))
* **wasmtime:** take the network's Protocol Family and Resolver from Config ([56cc53f](https://github.com/elct9620/kobako/commit/56cc53fa56cd987666f6b18b16e5264e314d8ef8))


### Bug Fixes

* answer an oversized datagram EMSGSIZE from the socket itself ([02317bc](https://github.com/elct9620/kobako/commit/02317bcc8e1ef86aa4b4a5e7a3b63b297cb6aeab))
* bound a guest's header list by size and charge it to its memory ([80c9ee2](https://github.com/elct9620/kobako/commit/80c9ee2523e0a3ad5de78e769a45136bcbcb6dc8))
* **filesystem:** answer a Bind's macOS stored name that is not UTF-8 as an illegal sequence ([6b68ede](https://github.com/elct9620/kobako/commit/6b68edee7f48797104fb9c14948ebd9a79548e63))
* **filesystem:** answer a Bind's stored name that is not UTF-8 as an illegal sequence ([ca755a6](https://github.com/elct9620/kobako/commit/ca755a65576698c17ade2da337956fb2ef187382))
* **filesystem:** open on Linux only the file a Bind judged ([8073129](https://github.com/elct9620/kobako/commit/8073129219ee49c6b6d4fe6a1199e316bf4bbfa5))
* **http:** refuse an outgoing body finished with trailers ([c3c8f1b](https://github.com/elct9620/kobako/commit/c3c8f1b3b6277865f119c04065109a8cb4e1ab76))
* **http:** send the trailers an outgoing body finishes with ([7792bcc](https://github.com/elct9620/kobako/commit/7792bccbedff4758b2ec85bae344c45769997518))
* **io:** answer a script's socket addr at the guest's own virtual address ([6e8891a](https://github.com/elct9620/kobako/commit/6e8891a9b242849f1d1d9cea825ab47480b69b0e))
* judge a request's authority, path, and headers as wasmtime does ([903cc7b](https://github.com/elct9620/kobako/commit/903cc7b39217f3038de6bab5981ea37499f25e1c))
* make each guest socket and its file at creation, as socket() does ([fca4311](https://github.com/elct9620/kobako/commit/fca4311060be90df2ec79c1d21bb3a9a5b0b36ca))
* **net:** answer EAGAIN on a shut UDP read, as non-blocking Linux does ([675376d](https://github.com/elct9620/kobako/commit/675376d971c721ca23a5bd581e7d83db59dc94aa))
* **net:** answer preview2 permits, options, and schemes as wasmtime-wasi ([3489166](https://github.com/elct9620/kobako/commit/3489166945852ca53626b82510f5c0a728cf0b26))
* **net:** bound an HTTP request by the timeouts request-options sets ([b0e35d8](https://github.com/elct9620/kobako/commit/b0e35d80a275ecb33679ff1967e05f476f155ce7))
* **net:** bound each wait a request leaves without a timeout at 600 seconds ([bd0fb84](https://github.com/elct9620/kobako/commit/bd0fb848904a57fff6dbd8eff413915dc2eecc8c))
* **net:** close a guest's TCP socket the way Linux closes one ([c0f95e0](https://github.com/elct9620/kobako/commit/c0f95e0410edd7498bb4a0cea0039cacef1253a4))
* **net:** dial an HTTP request's upstream through a TCP socket of the Invocation's own ([c4cb150](https://github.com/elct9620/kobako/commit/c4cb150f7cfe65db6cfa2cfc584002623b7b12cc))
* **net:** hold a request's parts against memory while it is sent ([d1b642b](https://github.com/elct9620/kobako/commit/d1b642b1171d54b4c0d4cbc1ae6e8ff313b3dc89))
* **net:** hold a request's parts and a response's head against memory ([fbc93c5](https://github.com/elct9620/kobako/commit/fbc93c5d763396cb5b928801380ef3989ec931af))
* **net:** hold the seam's read buffer against the Invocation's memory ([b589454](https://github.com/elct9620/kobako/commit/b5894545e572e34e17cce06d59bfa08ea1688555))
* **net:** judge a request's method in the case the guest sends it ([d74784a](https://github.com/elct9620/kobako/commit/d74784a464aa60493e3b1718eb9fd5dd47468c8a))
* **net:** judge and reach a connected peer the way the outbound path promises ([8a2797b](https://github.com/elct9620/kobako/commit/8a2797b867c886cf03cd123b1efc3011d7819bac))
* **net:** keep one virtual address per name, and judge a UDP send's peer by it ([8cc7afe](https://github.com/elct9620/kobako/commit/8cc7afe97956a25489bccfa82bf7d1ec7e750818))
* **net:** name a host socket's failure from its errno where std names none ([dddffa4](https://github.com/elct9620/kobako/commit/dddffa4bdc22575e3af4b27809662b04127aafc6))
* **net:** poll a TCP socket writable once its room is half what it queued ([a1873ee](https://github.com/elct9620/kobako/commit/a1873eead26a9a69f24bfe2d4d17ed9806d758cc))
* **net:** read a host the guest names as wasmtime-wasi's lookup does ([2defe86](https://github.com/elct9620/kobako/commit/2defe8666e9046179dab4ff8b74057e69c9ab10a))
* **net:** read back the very failure an upstream socket answered ([2c5a158](https://github.com/elct9620/kobako/commit/2c5a1588d73c1d858008c99fd7cda7f41bb9fac3))
* **net:** read EDESTADDRREQ and ENOPROTOOPT from a host socket's errno ([84f310e](https://github.com/elct9620/kobako/commit/84f310ee0e4102d6899d8144bc4b202ebe8b65cc))
* **net:** refuse a request whose target is not in origin form ([4d02252](https://github.com/elct9620/kobako/commit/4d02252ec6a5c4a714f99cc67f4f79bbab95558a))
* **net:** repoll a socket once, after the round that follows its request ([df2a0e2](https://github.com/elct9620/kobako/commit/df2a0e201442e3ebef4743fcc88e67597e1ead89))
* **net:** send a request's Host as the endpoint judged it ([30cb26d](https://github.com/elct9620/kobako/commit/30cb26d4c66e00e3fffeff7d4824658c1c10bb61))
* **net:** start a TCP socket's send buffer at Linux's default until it connects ([ea560c8](https://github.com/elct9620/kobako/commit/ea560c8ecdb0af143f94581ed10147bdc3e0359a))
* **net:** take a UDP peer's datagrams in before repointing, and refuse an empty label ([a67fa20](https://github.com/elct9620/kobako/commit/a67fa200521fe16379c0db5afb556268a0f606a7))
* **net:** take a UDP socket's datagrams in bounded turns, counting what each holds ([3362df2](https://github.com/elct9620/kobako/commit/3362df24070231ecaa8aec4f556309a02e5b3949))
* **net:** take a UDP socket's turn again once it spends its bound ([2711e0d](https://github.com/elct9620/kobako/commit/2711e0dd45290758bea28031c1cde5c5797ff593))
* **net:** try the next address when a request's first one is refused at once ([ac04578](https://github.com/elct9620/kobako/commit/ac04578bd77cb666433fda285587451fc66d5692))
* **net:** wait on a socket by its own flag, and reset a released one sent to ([9dbac06](https://github.com/elct9620/kobako/commit/9dbac065500e78c84a2ae000f06064f6f21b17f1))
* **net:** wait on a socket whose non-blocking flag was cleared ([ad36e6c](https://github.com/elct9620/kobako/commit/ad36e6c41011f3cebe4f3b653a2bef8c05dbbf21))
* **network:** answer an endpoint the ruleset refuses as the Host App's argument ([1c6d710](https://github.com/elct9620/kobako/commit/1c6d710855495f874761eef1933f51c93a7a62b0))
* **network:** bind a credential a Basic pair carries encoded ([7b40467](https://github.com/elct9620/kobako/commit/7b40467218d347ca588a55d573a4457231782a5a))
* **network:** compile a DNS endpoint like any other ([f47524b](https://github.com/elct9620/kobako/commit/f47524b888444314cd8762e45e4ef5ba4db92b64))
* **network:** judge a path rule in work bounded by the path ([20f37fc](https://github.com/elct9620/kobako/commit/20f37fcece42d3aea0018c987948a0814448b38e))
* **network:** never let an HTTP exchange go from inside its own task ([84b0ad9](https://github.com/elct9620/kobako/commit/84b0ad9ee5fca6be009e35cf7178503005d1b7f0))
* **network:** read a socket's own end at a virtual port, not the host's ([1ef8486](https://github.com/elct9620/kobako/commit/1ef84864b6f8106e25eaa5fd0abf7310d56835c6))
* **network:** refuse a denied path in another case or with a trailing slash ([ff727bc](https://github.com/elct9620/kobako/commit/ff727bc5307f9739a54878d45e2359868877e4e8))
* **runtime:** answer a TCP socket's error, close, and shutdown as Linux does ([dd4c684](https://github.com/elct9620/kobako/commit/dd4c68478c431833d79073eda8de83fde020ba81))
* **runtime:** answer a UDP port unreachable as refused on Windows too ([32a94c3](https://github.com/elct9620/kobako/commit/32a94c3991265a6e967253d7d624c27282acc9c4))
* **runtime:** charge empty datagrams and virtual addresses ([9a4d9ba](https://github.com/elct9620/kobako/commit/9a4d9ba92cbd3ba5f7570f1693282202db43b6d9))
* **runtime:** charge the guest's growth and host-held memory to one budget ([f19659e](https://github.com/elct9620/kobako/commit/f19659ed93d0f5b0c1472dbc3bdbbb6815efbb4f))
* **runtime:** check a socket's state before its connect is translated or judged ([044f471](https://github.com/elct9620/kobako/commit/044f471f82aa5ac9edae39fae6dfae31be4e3dbf))
* **runtime:** class IPv6 addresses by IANA's special-purpose registry ([205ae65](https://github.com/elct9620/kobako/commit/205ae65e1924d65087e33472ba0074c840109932))
* **runtime:** connect a TCP socket again as Linux does ([7679d71](https://github.com/elct9620/kobako/commit/7679d71bd2bd71268b7da10a210be9522b032347))
* **runtime:** follow Linux's inet socket through a connect that fails ([596febd](https://github.com/elct9620/kobako/commit/596febdbce08a45f5fc49b713977af5acc664702))
* **runtime:** give a socket its virtual port as Linux's inet layer does ([2c80c02](https://github.com/elct9620/kobako/commit/2c80c027801b5a00501fb8141e8ebde53879a855))
* **runtime:** give back a TCP socket's unnamed port once a connect fails ([03a5d7f](https://github.com/elct9620/kobako/commit/03a5d7fda454ba42a6a6798c1698b282be1c0f4e))
* **runtime:** give each Invocation's outbound seam its own TLS sessions ([75119d8](https://github.com/elct9620/kobako/commit/75119d88a49f546a2d827a9476c5067d99bc02dd))
* **runtime:** give each real address its own virtual address and dial one destination ([71c1b8d](https://github.com/elct9620/kobako/commit/71c1b8d7c3ee5ee34f57e5f1a48b43619176d129))
* **runtime:** hold a socket's buffers against the Invocation's memory ([5d629a7](https://github.com/elct9620/kobako/commit/5d629a71f1940222dac36a57502030025575ef70))
* **runtime:** hold a TCP socket's virtual port before it dials ([dff5cfe](https://github.com/elct9620/kobako/commit/dff5cfe788339146fda4ef3cb9b88c2cb6bd0302))
* **runtime:** judge a connect in Linux's order inside the runtime ([f5ede3a](https://github.com/elct9620/kobako/commit/f5ede3af5e8329c21f7630066c1ec539863ef433))
* **runtime:** judge IDN endpoints, Basic credentials, and request order as promised ([a1d4a35](https://github.com/elct9620/kobako/commit/a1d4a353f439cfa2fc5a9b11119f0bda622122ef))
* **runtime:** judge the guest's HTTP headers, binds, and peerless streams in the runtime ([bc3e024](https://github.com/elct9620/kobako/commit/bc3e024483e43d8b121cb67e661e06e99997f604))
* **runtime:** judge the request target the server reads ([9bc185e](https://github.com/elct9620/kobako/commit/9bc185e6f00375498f4d9119a34357e32d52279b))
* **runtime:** keep a failed growth in the memory peak ([2e2b986](https://github.com/elct9620/kobako/commit/2e2b986dfa47aff61f0c20ad9b63178bfccbd441))
* **runtime:** keep a name a Mock holds twice held until both holds drop ([26b362c](https://github.com/elct9620/kobako/commit/26b362cc9e61509f025ac6c5700bf3e95ed2b137))
* **runtime:** keep a refused connect's error when its options cannot be set ([258b51e](https://github.com/elct9620/kobako/commit/258b51e1d8398619da5cd6a1dcbd8e0ba42cf7d6))
* **runtime:** keep a TCP socket's options when its host socket refuses one ([53284a4](https://github.com/elct9620/kobako/commit/53284a4ad02da8dd23dd9402b070462f58b765aa))
* **runtime:** keep a UDP socket's port as it connects again ([070cf8a](https://github.com/elct9620/kobako/commit/070cf8a50c6b8cc93816b5a34711c3910d13333e))
* **runtime:** keep Nagle's algorithm on a guest's own TCP socket ([7db0695](https://github.com/elct9620/kobako/commit/7db06959f615300783b606a3cb7416c8b909980c))
* **runtime:** let an HTTP exchange go once nothing holds it ([2d78df9](https://github.com/elct9620/kobako/commit/2d78df98d11cdd8671629b1da2a5ac4b0b1862e7))
* **runtime:** name a host failure through one table for files and sockets ([1490a1f](https://github.com/elct9620/kobako/commit/1490a1f7c5fdfa5a383a12bfb0e0b8a4fef3fcdd))
* **runtime:** name a local address another socket holds as EADDRINUSE ([6b39011](https://github.com/elct9620/kobako/commit/6b3901117e18568bd1c47d3f1a913df67f09ea35))
* **runtime:** read a UDP socket's own queue alone, leaving the rest to the reactor's next turn ([b1a9208](https://github.com/elct9620/kobako/commit/b1a92088e56c2f9f934aa4e7f6963f497d30c456))
* **runtime:** refuse a cap of zero in one place both frontends ask ([aef531e](https://github.com/elct9620/kobako/commit/aef531eda4f2b32bc4748b0d09b98058331af3b6))
* **runtime:** refuse a connect to an unmapped virtual address ([6a65311](https://github.com/elct9620/kobako/commit/6a653119a14d428df421157accf76ecb2ec04efb))
* **runtime:** refuse a connect when the host's addresses are unread ([47b1cd0](https://github.com/elct9620/kobako/commit/47b1cd0d168a6baa8c94bc5da85513bd3dc7e878))
* **runtime:** refuse a path servers normalize differently ([ba45e4f](https://github.com/elct9620/kobako/commit/ba45e4f5a48bf8f28e4e812465ad122bcbd8f72d))
* **runtime:** refuse a second connect on a connecting or connected socket ([2949d40](https://github.com/elct9620/kobako/commit/2949d402303e11af2b4346a866fee754e1e00dbd))
* **runtime:** refuse an https request with no trust roots before dialing ([ba0995c](https://github.com/elct9620/kobako/commit/ba0995cd6dc3f5f9f6b0e74e8b0801348f35ebb5))
* **runtime:** report socket failures as Linux does and keep the reactor and resolver honest ([b7e9a0d](https://github.com/elct9620/kobako/commit/b7e9a0dfc55f32bbe2a844ff89c0bc4162429438))
* **runtime:** resolve a request's authority as a lookup holding its place ([fd62ff6](https://github.com/elct9620/kobako/commit/fd62ff6ba9a53ab66eb8a68037ae85bb95f01387))
* **runtime:** send a UDP datagram at once and answer its failures as Linux does ([6abbab0](https://github.com/elct9620/kobako/commit/6abbab0e0da88f4b05797396aa7c96b8e061b3ed))
* **runtime:** shut a released socket's host side down even when the release races its connect ([1d41d66](https://github.com/elct9620/kobako/commit/1d41d66370fd28af0c760ae8a72fc48b83f26b82))
* **runtime:** take a datagram past the receive buffer into an empty queue ([8f2f670](https://github.com/elct9620/kobako/commit/8f2f670619c7997e0c75a7869e1d5592b4c0e50a))
* **runtime:** take the reactor before a TCP socket dials ([e056522](https://github.com/elct9620/kobako/commit/e0565227c357d05bca12610cd26d4aaac85b604a))
* **vfs:** find a directory's inode in use by its identity ([d817440](https://github.com/elct9620/kobako/commit/d817440b61ca50a192f0f8f1b3d1fd03fe80595d))
* **vfs:** keep a dentry a move carried while its revalidation was asked ([cb20bb6](https://github.com/elct9620/kobako/commit/cb20bb6658d59e000d07fe6e9e299a0a0c5d3f35))
* **vfs:** reach a nested mount however the guest spells its path ([73fb96a](https://github.com/elct9620/kobako/commit/73fb96ab63eaf01c73aaef1c803424513b2b07fd))
* **vfs:** stand a nested mount on its mount point's dentry ([9ae2dd8](https://github.com/elct9620/kobako/commit/9ae2dd8b2403bd62faa62208d05a704d00de709a))
* **wasmtime:** answer a connect, a short write, and a datagram's sender as wasmtime-wasi ([d1762d0](https://github.com/elct9620/kobako/commit/d1762d0a6f703c003cdb8bf00c81628f61ce2b6f))
* **wasmtime:** give back the charge of a growth wasmtime then fails ([070c463](https://github.com/elct9620/kobako/commit/070c463e11428eb27465d6d9a9e7ba38b738249f))
* **wasmtime:** judge a TCP socket's pollable by the connect under way as it is polled ([b89bccd](https://github.com/elct9620/kobako/commit/b89bccdba789d69dc7b69ad07a7176b0d310b963))
* **wasmtime:** refuse a short socket write whole, before any byte leaves ([5be1ec0](https://github.com/elct9620/kobako/commit/5be1ec027474376ecfcadce6d280891efe1a0afa))


### Code Refactoring

* **runtime:** judge each Grant's requests through one generic Policy ([770fc66](https://github.com/elct9620/kobako/commit/770fc665b00532f4092dfdb8a864e02d387065c3))
* **runtime:** name the errno type Errno at the crate root ([f608e31](https://github.com/elct9620/kobako/commit/f608e31e81d1be9ddc6bb2fc8f6a26e5acafced4))
* **sdk:** hold a Sandbox's caps as the runtime's Limits ([9730d6a](https://github.com/elct9620/kobako/commit/9730d6a73fec595c2b75ac8981aeb655937045df))
* **transport:** name the message cap by what it bounds ([133e1c1](https://github.com/elct9620/kobako/commit/133e1c151506a1f41d5f2d17ac2678648c6ed262))

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.19.0...kobako-runtime-v0.20.0) (2026-11-07)


### ⚠ BREAKING CHANGES

* **runtime:** Mount::filesystem is removed; read mount.mnt().sb() in its place.
* **runtime:** Policy::allows is asked Access::ReadWrite before an open that may create finds what stands, so an open of an existing file asking to create is asked ReadWrite and then the access it opens for, where it was asked that access alone.
* **runtime:** Kstat's len, accessed, modified, and changed fields are now size, atime, mtime, and ctime, and Profile::guest_metadata is now
* **runtime:** spell each file type as Linux's FT_* names it
* **runtime:** name what a path reports Kstat, as Linux's struct kstat
* **runtime:** build an open's flags once and hand them to do_file_open
* **runtime:** answer EAGAIN to a `..` a move landed before
* **runtime:** judge a path through a link where the link leads
* **runtime:** kobako_runtime::Denial is gone. A Denial::ReadOnly is now
* **runtime:** name a Filesystem's make-and-open atomic_open
* **runtime:** Overlay::with_blocking is gone, and kobako_runtime::filesystem::Blocking moved to kobako_runtime::sched::Blocking. Call kobako_runtime::sched::register_blocking once at startup instead.
* **runtime:** Mount::allows_writes is gone. Ask mount.mnt().is_readonly() for what the VFS checks.
* **runtime:** kobako_runtime::filesystem::Access moved to kobako_runtime::mount::Access. kobako::Access and kobako_runtime::Access are unchanged.
* **runtime:** a mount's block or Policy no longer allows writing beneath a read-only Mount, and access defaults to read. Pass access: :read_write in Ruby, or Access::ReadWrite in Rust, where the block should decide about changes. A change beneath a read-only Mount answers EROFS without asking it.
* **runtime:** File::metadata and File::set_len are gone from what an open file answers. Implement InodeOperations::getattr and setattr, answering from what the file holds where the entry must still answer once its name is gone.
* **runtime:** File::read_at is FileOperations::read_iter, and File::write_at and File::append are FileOperations::write_iter, which take a Kiocb for the offset. A write finding Kiocb::append set lands at the end and answers Unsupported if it cannot take that as one step; each moves Kiocb::pos past what it took.
* **runtime:** File::close is FileOperations::flush and takes &mut self. Move what a close reports there, and what a file lets go of to Drop.
* **runtime:** File::sync is FileOperations::fsync, and a file that does not implement it now answers EINVAL instead of success. Implement fsync returning Ok where a file has nothing to flush.
* **runtime:** the File trait is FileOperations. Rename it where a Filesystem implements it.
* **runtime:** the Filesystem trait is InodeOperations, whatever earlier notes call it. Each entry answers through its own: a lookup returns a Lookup, whose Inode holds them, and an operation an entry lacks answers None.
* Inode::stored_name is replaced by Inode::lookup, which the runtime asks before keeping a name's dentry. A rename that changes only a name's case on a host folding case leaves it as it was, as a rename of an entry onto itself does.
* Inode::rename takes the source, the target directory, and the target, without a Rename; Rename and Rename::check are removed. A Filesystem written in Ruby answers rename(from, to) instead of rename(from, to, directory:).
* **runtime:** a read through an Overlay from inside one of its own reads now answers instead of raising EDEADLK; only a change does.
* **runtime:** an Overlay's upper Tmpfs keeps its changes beneath upper/ and work/, whiteouts are devices rather than .wh. files, and names starting with .wh. are ordinary names.
* **runtime:** Overlay::new and Overlay.new refuse an Overlay over an Overlay over an Overlay, and more than 500 lower Layers.
* Overlay::new() and Overlay::layer are replaced by Overlay::new(lower, upper), which refuses an upper that is no Tmpfs. In Ruby, Overlay.new.layer(fs, access:) is replaced by Overlay.new(lower: [...], upper: tmpfs).
* **runtime:** the nofollow field of OpenOptions is removed, and Inode::open refuses a link standing at the name as FilesystemLoop. A Filesystem of your own drops the field and refuses a link there; a caller that relied on an open following one walks to the target first, as the runtime does.
* a Filesystem is the instance a Mount places, made with Filesystem::new from its root Inode. Mount::new, the Rust Sandbox's and Context's mount and mount_with_policy, and Overlay::layer take one, and
* Kobako::Filesystem::Memory is Kobako::Filesystem::Tmpfs with size: and nr_inodes: in place of limit: and inodes:, and Kobako::Filesystem::Directory is Kobako::Filesystem::Bind. In Rust, Memory is Tmpfs with with_size, with_nr_inodes, and DEFAULT_SIZE; Directory is Bind; and the Filesystem trait is Inode.
* Overlay::layer takes the instance a Filesystem is mounted by instead of the Filesystem itself.
* the Filesystem trait's open, open_dir, symlink_metadata, stored_name, read_link, create_dir, remove_dir, remove_file, and rename take a &Dentry instead of a name; open_dir_nofollow is now open_dir, and metadata() has no default. The kobako crate re-exports Dentry.
* the Filesystem trait's metadata(path) is removed; metadata() reports the directory itself and defaults to what symlink_metadata answers for the empty path.
* the Filesystem trait's open_dir is removed; open_dir_nofollow opens a directory by name and refuses a link.
* Mount::new, and the Rust Sandbox's and Context's mount and mount_with_policy, take a shared instance made from the Filesystem instead of the Filesystem itself.
* Mount's check, guest_path, moves_directories, and follows_links are no longer public; an engine calls the vfs operations, which ask the Mount itself.
* the Filesystem trait's canonicalize_beneath and for_policy are removed and read_link is added; a Filesystem holding no links answers read_link with InvalidInput. The Mount's for_invocation is removed, and a Directory no longer has a mode following no link.
* the Filesystem trait's hold_directory and the Hold type are removed; a directory a Filesystem opens holds what it needs open itself. Beneath a Policy, a directory opened on a Filesystem that reports no inode number now answers ENOENT.
* **runtime:** a Filesystem implementation answers open_dir, open_dir_nofollow, entries and stored_name in place of read_dir and stored_path, and its rename takes the target directory before the target path.
* Kobako::Filesystem::Temporary and the Rust Temporary are removed; mount a Memory, and mount a new one through the Context where each run should start empty. A Filesystem implementation drops its for_invocation method.

### Features

* answer every path call from the runtime's VFS ([39d0292](https://github.com/elct9620/kobako/commit/39d0292c3b5d3ec8ea563fe4720c8449e92ae86a))
* ask a Filesystem about a directory itself, never through a link ([5881b87](https://github.com/elct9620/kobako/commit/5881b87d8e06cdabd12fe958c65f5bc67b24375d))
* check what a rename moves in the VFS, as Linux does ([dd5e267](https://github.com/elct9620/kobako/commit/dd5e267b7d3f09591864575ce8b16aaf2f703a07))
* hand a Filesystem the dentry the VFS made for each name ([5a2f85a](https://github.com/elct9620/kobako/commit/5a2f85a030918f329b98874af843385ea5ba12a8))
* hand every invocation the Filesystem its Mount holds ([17bb576](https://github.com/elct9620/kobako/commit/17bb576803f8366a073993efa17df788949f8a69))
* key each dentry by the name its directory stores, as Linux's d_add_ci ([fc0267b](https://github.com/elct9620/kobako/commit/fc0267b9df310482842e0695d0350b7dc7f3a65f))
* layer an Overlay over the instance its Filesystem is mounted by ([c321548](https://github.com/elct9620/kobako/commit/c321548d294698d4b8f16336c0b035f0ee909511))
* let no Filesystem follow a link to open a directory ([79a6a0b](https://github.com/elct9620/kobako/commit/79a6a0bbec78e273f218e863e2c22a3bb960b4de))
* mount a Filesystem instance whose Mounts share what moves ([bead3f8](https://github.com/elct9620/kobako/commit/bead3f84ebd9f4a8b240afc52aa7e0967a3a400a))
* name the Filesystems kobako ships after the Linux ones they mirror ([4fb367b](https://github.com/elct9620/kobako/commit/4fb367beefedf89ee424ff32b33837c000760f4b))
* name the instance a Mount places a Filesystem ([50ca78e](https://github.com/elct9620/kobako/commit/50ca78e405e3cd567095b49c77d3b26b2d104c57))
* report a file's link count and times to the guest ([5e95113](https://github.com/elct9620/kobako/commit/5e95113a056bd676bd5736f6ffa79f210a036647))
* **runtime:** answer every Filesystem call from a directory handle ([5a4376c](https://github.com/elct9620/kobako/commit/5a4376c6a8d3ede3272f8d6393ab27589f733eeb))
* **runtime:** ask a Filesystem whether a held dentry still names its entry ([13f56a4](https://github.com/elct9620/kobako/commit/13f56a40bd9622ba15ee6b34dc93393c92b3e270))
* **runtime:** ask a Filesystem's permission where Linux's VFS does ([2786574](https://github.com/elct9620/kobako/commit/2786574efe2401f98fa8650848ee09c4be0335aa))
* **runtime:** bound how many Layers an Overlay stacks, as Linux does ([1c88f06](https://github.com/elct9620/kobako/commit/1c88f063d7664c9a020083df863b33b66df737c8))
* **runtime:** find where a walk starts once its name is taken ([2a61459](https://github.com/elct9620/kobako/commit/2a6145916dd2dfda0a8ee09baebd3a6dfa0069e8))
* **runtime:** follow an Overlay directory through its moves ([858d5d7](https://github.com/elct9620/kobako/commit/858d5d75dbb2246475a0f3938b527a0555cc3ab9))
* **runtime:** judge a path through a link where the link leads ([7407a71](https://github.com/elct9620/kobako/commit/7407a71cfbcb9c4f5faa3ef954c592d0687174cc))
* **runtime:** keep the times a caller sets on a Tmpfs, a Bind, and an Overlay ([d53d547](https://github.com/elct9620/kobako/commit/d53d5479c48e4d423addf02b7d373784409d1604))
* **runtime:** let a caller ask an open file what kind it is ([9ab1182](https://github.com/elct9620/kobako/commit/9ab1182608eab829c778efa531ce0f316583b4fd))
* **runtime:** let a file answer its own seek ([fee742a](https://github.com/elct9620/kobako/commit/fee742addc2cbcd04e4cca387b399b419a2eab5d))
* **runtime:** let a Filesystem hand one inode for many names ([fc34fc6](https://github.com/elct9620/kobako/commit/fc34fc6ff7dfb33daa492cb5df390b9615ca4d04))
* **runtime:** let a Filesystem make a file through create ([58ab067](https://github.com/elct9620/kobako/commit/58ab06766daa6d173e730941efcd11acc21ed9d5))
* **runtime:** let an ABI check a path's name and an open's flags first ([482e9aa](https://github.com/elct9620/kobako/commit/482e9aaa2181e78cce091d7a546f6f5484270eb9))
* **runtime:** lock an Overlay as Linux locks overlayfs, so its calls run at once ([b547dcd](https://github.com/elct9620/kobako/commit/b547dcd1abb3c33b74b0359692443e812fcb06ec))
* **runtime:** lock each directory a call changes, as Linux does ([126694b](https://github.com/elct9620/kobako/commit/126694b826c430a4b62079ddc4872873e743ef12))
* **runtime:** mark a file a stream from what its Filesystem opened ([7704fa8](https://github.com/elct9620/kobako/commit/7704fa8df9bccb29a32b36304a537904af5226d9))
* **runtime:** mark and copy up an Overlay's changes as overlayfs does ([4ea3f5f](https://github.com/elct9620/kobako/commit/4ea3f5f319d79314e1e4a80df32e6ff094bad377))
* stack an Overlay as overlayfs does, under an upper Tmpfs ([2b59780](https://github.com/elct9620/kobako/commit/2b597806809d7a810d29265e9e377d7d648d77f9))
* walk a guest path one name at a time and follow links there ([2e57fdc](https://github.com/elct9620/kobako/commit/2e57fdc11c5cbeb00697e9b287f59c15f0c91e2d))
* **wasmtime:** judge every invocation by the security modules a Driver is built with ([ba42b85](https://github.com/elct9620/kobako/commit/ba42b85f43ba8eb4bacba807a4c5af1788a614ae))


### Bug Fixes

* keep an opened directory's identity through the directory itself ([28ee9e0](https://github.com/elct9620/kobako/commit/28ee9e003a30fb9159ff3f6c47dce2da25586eb2))
* **ruby:** list what a Ruby Filesystem still holds and show its links as links ([b1097e7](https://github.com/elct9620/kobako/commit/b1097e7ebec3905224135775f96c0cf8c134daa8))
* **runtime:** align an Overlay's answers with overlayfs's order ([ccfb7d4](https://github.com/elct9620/kobako/commit/ccfb7d46c2d4889cc837746f3a82345f27dde8b1))
* **runtime:** answer a Bind dentry stale once its host moved what it named ([cb94c51](https://github.com/elct9620/kobako/commit/cb94c514f8568d619fc3e8d97efb798a6649568c))
* **runtime:** answer a dot path or a slash create through a link as Linux ([8aafe42](https://github.com/elct9620/kobako/commit/8aafe42cd517cdc9730ac41b092000570c065511))
* **runtime:** answer a link call by what stands, as filename_linkat does ([5a0ac25](https://github.com/elct9620/kobako/commit/5a0ac257a260f8638caf794df50d95ad11fb139d))
* **runtime:** answer a move's flags and a link's source where Linux does ([be7af48](https://github.com/elct9620/kobako/commit/be7af48aa4dce202f9a2d1342a3cd0e314240cd4))
* **runtime:** answer about a name through DentryOperations ([ae6ca8a](https://github.com/elct9620/kobako/commit/ae6ca8abaebdbfd289378ba9299a6446bb611e02))
* **runtime:** answer an Overlay file from where its data stands now ([024f274](https://github.com/elct9620/kobako/commit/024f274904b9fb0a9c09a05a4b5d40e1558b65a3))
* **runtime:** answer an Overlay's merged directory and long names as overlayfs does ([57b04dd](https://github.com/elct9620/kobako/commit/57b04dd210fa56fb9478e6568ca992d55c4d1a8a))
* **runtime:** answer each path call in Linux's order ([85d0407](https://github.com/elct9620/kobako/commit/85d040761699ca72d3a1e2ec5737d9f9ccfcbd47))
* **runtime:** answer EAGAIN to a `..` a move landed before ([1aa66dd](https://github.com/elct9620/kobako/commit/1aa66dda78bd63aff91a78bfe8d6bff6306a9c19))
* **runtime:** answer ENODEV to an allocation on a device ([0ba099b](https://github.com/elct9620/kobako/commit/0ba099b474890179e3c4c4e8a13837b9be5b5ead))
* **runtime:** answer ENOENT for a missing last name before the walk's bounds ([db47ae3](https://github.com/elct9620/kobako/commit/db47ae349bda70b365fade60af55edd237ba16b3))
* **runtime:** answer what a Filesystem says when a directory will not open ([5ef653c](https://github.com/elct9620/kobako/commit/5ef653c35f222fbd0e78c906551b2880b4d62f7d))
* **runtime:** ask a directory moving to another parent whether it may change ([138dcb0](https://github.com/elct9620/kobako/commit/138dcb0836074771ce83d58248fe7b0f7ebf5b99))
* **runtime:** ask a Filesystem once for a name walks reach at once ([565c6db](https://github.com/elct9620/kobako/commit/565c6dbed81681e142531cfb19a1c8a47f82aee6))
* **runtime:** ask a walk's start whether it still stands under its directory's lock ([4b14e4e](https://github.com/elct9620/kobako/commit/4b14e4e91aaf1a91d2815789ed41c2da9c50e97d))
* **runtime:** ask an entry about itself however a guest reached it, as fstat and ftruncate do ([1166426](https://github.com/elct9620/kobako/commit/1166426d3f4bc27d9c2d0d3a8f2b7f1a6a23f59e))
* **runtime:** ask atomic_open for a directory where a name ends in a slash ([667c4a8](https://github.com/elct9620/kobako/commit/667c4a8b302a53af3876377cabf5ed8454673be7))
* **runtime:** ask atomic_open for an open's last name no walk looked up ([9860ae9](https://github.com/elct9620/kobako/commit/9860ae9391874cf27bf63345b4672458f696a8a8))
* **runtime:** ask each Layer's permission before an Overlay reaches into it ([e7bc739](https://github.com/elct9620/kobako/commit/e7bc7396ae8b0b584c44e592213755788053af61))
* **runtime:** ask only the dentries an Overlay's entry keeps, and name each helper for what it does ([bb5613c](https://github.com/elct9620/kobako/commit/bb5613c33d1d4e20640fba3c4a1a605fd82cade1))
* **runtime:** ask sb_permission before a Filesystem's permission ([a2b3fd8](https://github.com/elct9620/kobako/commit/a2b3fd8a86a7af39be37751936ec36f061fcb7d6))
* **runtime:** ask the readlink hook only of a link ([d584cbb](https://github.com/elct9620/kobako/commit/d584cbb66d34672f49cb44a0226071277a029275))
* **runtime:** ask to make a file before an open finds what stands ([0b3ecf5](https://github.com/elct9620/kobako/commit/0b3ecf54c6975576dee13decb1904ccb371f30be))
* **runtime:** begin a walk once a move under way lands ([92efa17](https://github.com/elct9620/kobako/commit/92efa17539b39985fca03a0b74235670d597eb73))
* **runtime:** bind an Overlay entry's Layers as its lookup finds them ([b76c5a2](https://github.com/elct9620/kobako/commit/b76c5a226059299f97bf607b7d4e0796e4b232ec))
* **runtime:** bound the host directories a Bind keeps open ([531cf3a](https://github.com/elct9620/kobako/commit/531cf3a6f84bc706d00a9eb91bdafb4f76962f6e))
* **runtime:** build a security hook's name from the dentry it is about ([f9c901a](https://github.com/elct9620/kobako/commit/f9c901abf9168d49de8aa405265a65519c047631))
* **runtime:** build an open's flags once and hand them to do_file_open ([6672734](https://github.com/elct9620/kobako/commit/6672734fc42af8c2040402c8ed2b14d562c31328))
* **runtime:** carry a call's link count into the walk made again for ESTALE ([229846d](https://github.com/elct9620/kobako/commit/229846df50f509dca5a03d41601b8e92a620994a))
* **runtime:** charge a listing's snapshot when it is read, before any entry ([a5eb698](https://github.com/elct9620/kobako/commit/a5eb69823265729683949b569271507e6a5329a1))
* **runtime:** climb an Overlay's directories in a loop ([70706f3](https://github.com/elct9620/kobako/commit/70706f34d121e335d64cc05fe255cd67f67af614))
* **runtime:** copy a file up before setting its times through an Overlay ([1a20b59](https://github.com/elct9620/kobako/commit/1a20b59c0db975ebe2bc36f7d5a051b64f325940))
* **runtime:** copy up from what an Overlay file's inode keeps ([ce9abb5](https://github.com/elct9620/kobako/commit/ce9abb52e1a6402305dad5214a3a321bdc615bd9))
* **runtime:** count a held listing by what it holds on the heap ([8425dab](https://github.com/elct9620/kobako/commit/8425dabd1cdf19450d3fff0510850972e3868633))
* **runtime:** count no name for a Tmpfs directory that was removed ([bccbd99](https://github.com/elct9620/kobako/commit/bccbd992f2de8e23496a2cf81a33e4653d18ce31))
* **runtime:** count no names for a Tmpfs file nothing holds any more ([20c96be](https://github.com/elct9620/kobako/commit/20c96bec2534d005b84e931025ded3cf6be69c08))
* **runtime:** decide what stands beneath a directory before taking the LRU ([7d6da0d](https://github.com/elct9620/kobako/commit/7d6da0df3cffbd29138a65e0e3adc7031c8386e1))
* **runtime:** derive an open's directory and append asks in build_open_flags ([a23e33e](https://github.com/elct9620/kobako/commit/a23e33e82114fb317314149950809be0d99b510f))
* **runtime:** empty a file an open truncates through setattr ([0ee006f](https://github.com/elct9620/kobako/commit/0ee006f4683ed3059a9f3251d450bf3de0eb365d))
* **runtime:** end a wait for a lock once its run is to stop ([4cfba33](https://github.com/elct9620/kobako/commit/4cfba3308008cdf23ecb368620e78619f9691fa7))
* **runtime:** find a move's names once both directories are held ([4803a12](https://github.com/elct9620/kobako/commit/4803a12216770e6e348ed33dc6711334aeb6ca9b))
* **runtime:** find and create an O_CREAT open's file under one directory lock ([a3684d4](https://github.com/elct9620/kobako/commit/a3684d4ca79e7b8ae518118adf9ec965ba46ba9a))
* **runtime:** free a Tmpfs directory's inode when its last reference goes ([3a0d970](https://github.com/elct9620/kobako/commit/3a0d970fb238ea9a56d36c176ac78fda586b411e))
* **runtime:** hand a listed entry where it stands, as dir_emit does ([804f8b2](https://github.com/elct9620/kobako/commit/804f8b2ce93a8d5b935d400d295ba5f2ce7c5a32))
* **runtime:** hand a stream's read and write no position and keep none ([a878d51](https://github.com/elct9620/kobako/commit/a878d51539a062e48faf7811b884d1320cc772d1))
* **runtime:** hand atomic_open no truncation an open cannot carry out ([70c8ad7](https://github.com/elct9620/kobako/commit/70c8ad741b04905259cf11327cab3a6043293d53))
* **runtime:** have an open asking O_SYNC carry O_DSYNC as Linux does ([035be21](https://github.com/elct9620/kobako/commit/035be21b633b469825faa69ce764347a68ca0358))
* **runtime:** hide a name as an Overlay's upper holds it when the removal lands ([14a84c6](https://github.com/elct9620/kobako/commit/14a84c62fde74c7347cf16bc2002104022e64ad0))
* **runtime:** hold a Bind's directory descriptor to what its dentry recorded ([f9e7fc9](https://github.com/elct9620/kobako/commit/f9e7fc99ab37a7f41eb67661bbf7c8c1466ff090))
* **runtime:** hold a directory the guest opens from the moment it opens ([1dcca08](https://github.com/elct9620/kobako/commit/1dcca089ffc5ce8459940565999ca09b20bee44a))
* **runtime:** hold an Overlay directory's Layers instead of reaching them again ([85d5539](https://github.com/elct9620/kobako/commit/85d5539ee0ad6c98e0b20e376f540fcaf704ac16))
* **runtime:** hold what a guest's listings keep within its memory limit ([e4d74a5](https://github.com/elct9620/kobako/commit/e4d74a5ad2d5554e1b509ee606c1911811f66be8))
* **runtime:** judge a time set through a descriptor where it stands ([e1e5147](https://github.com/elct9620/kobako/commit/e1e51475c63ea20593b28b86fdc59fce7e51e019))
* **runtime:** judge a truncate by what the security module kept at the open ([754fb8d](https://github.com/elct9620/kobako/commit/754fb8d8a467ae1e7802deee48735693cf9eaa0d))
* **runtime:** judge an allocation or advice by what the file is ([76f9b87](https://github.com/elct9620/kobako/commit/76f9b87c6bf66294009fb2cef9ebb7a9dc50f9b7))
* **runtime:** keep a Bind directory's host descriptor on its dentry, walked as 9p walks ([8ce61ef](https://github.com/elct9620/kobako/commit/8ce61ef074d194db0e250e88c98a5afceab27152))
* **runtime:** keep a Bind's walk and allocation answers true under a race ([cce1a2a](https://github.com/elct9620/kobako/commit/cce1a2a0e64bf44648e01caf14e901f1cb04fad3))
* **runtime:** keep a Cred without kobako's rules inside the runtime ([775bb5c](https://github.com/elct9620/kobako/commit/775bb5c09c1fb8ce4412208df8113e26260e89c9))
* **runtime:** keep a lower Layer's hard links two entries of an Overlay ([0444467](https://github.com/elct9620/kobako/commit/04444670df2d708d6b227ed35a04e80ed8607d3d))
* **runtime:** keep a moved Tmpfs entry's names through the move ([fe42277](https://github.com/elct9620/kobako/commit/fe422775bcca9ae09e262855ea27f535368ce5e1))
* **runtime:** keep a name nothing stands at, as each Filesystem decides ([8ecee7e](https://github.com/elct9620/kobako/commit/8ecee7ebbed65fa3c7415e98043d3b130af5971a))
* **runtime:** keep an Overlay's copy up to what it made itself ([d16f9e4](https://github.com/elct9620/kobako/commit/d16f9e4efae77289701d5976c947f7626674bf73))
* **runtime:** keep an Overlay's view of a directory it emptied ([e869247](https://github.com/elct9620/kobako/commit/e8692472ed50690882ee85cc41816cd60f6003be))
* **runtime:** keep both files of a Tmpfs exchange named ([fa50c7c](https://github.com/elct9620/kobako/commit/fa50c7c6fde0c4e0cd382d358457e53212eab9d4))
* **runtime:** keep no dentry a walk let go of for the next walk ([ed4e723](https://github.com/elct9620/kobako/commit/ed4e7239a3ccde2ada757427368a8ec282a21745))
* **runtime:** keep one dentry for a directory a Filesystem names twice ([9541b7a](https://github.com/elct9620/kobako/commit/9541b7a79a15362bb3254b093ea76dd17ad0bcc4))
* **runtime:** keep one dentry for a directory two walks find at once ([18d9b26](https://github.com/elct9620/kobako/commit/18d9b26d72490cf2bec007839be1eabfe074f933))
* **runtime:** keep pipes among the Filesystems, reading nothing into no room ([79a2240](https://github.com/elct9620/kobako/commit/79a22401f9c8ab3c8fe777955a4b39e880418686))
* **runtime:** keep the flags a directory was opened with ([e328aa4](https://github.com/elct9620/kobako/commit/e328aa4d6ef59aaef970d3b9e162f5edeb9d6222))
* **runtime:** keep what preview1's rights mean out of the VFS open ([fd78dce](https://github.com/elct9620/kobako/commit/fd78dce168149cb439f65ab0db74f0b655b9e35f))
* **runtime:** key the inode number a permissive guest reads ([1d2b8b2](https://github.com/elct9620/kobako/commit/1d2b8b276c805aba557b336bae6cca31fd330570))
* **runtime:** learn what every looked-up name names, so a moved Bind directory is never reached by its old path ([773498a](https://github.com/elct9620/kobako/commit/773498a0b7dac76b30d482d48d82cc5a1da5e811))
* **runtime:** leave a file where it stands after a write of nothing ([ca36bb4](https://github.com/elct9620/kobako/commit/ca36bb463240ccfbf5bad114c4060f90f92f7a29))
* **runtime:** leave a move between two names of one entry alone ([717d321](https://github.com/elct9620/kobako/commit/717d3214ffc7a0a11527898a1b0c067f6d53b42d))
* **runtime:** leave a moved start's retry to the caller ([8faf08e](https://github.com/elct9620/kobako/commit/8faf08e5af9c7be0ce6cf3314be7afc8e9a482d9))
* **runtime:** leave a walk where it stands at a link pointing nowhere ([3f98a6f](https://github.com/elct9620/kobako/commit/3f98a6f52cd6e4f04415d4860bbcd3ddcb38255e))
* **runtime:** leave a walk's ESTALE to the walk that made it ([4aee35e](https://github.com/elct9620/kobako/commit/4aee35e2468a8be2b252b2a0da17d562b908cd14))
* **runtime:** leave the descriptor table to the Driver, as FD_ADD reserves first ([12e6b65](https://github.com/elct9620/kobako/commit/12e6b65b4b47bdd4e73465f904c9cef92c27e1bd))
* **runtime:** let a descriptor's stat and listing ask no hook ([48d265a](https://github.com/elct9620/kobako/commit/48d265ab83773e1ee436f06d4f822bf57f372ba2))
* **runtime:** let a directory answer as one open to read does ([3919be4](https://github.com/elct9620/kobako/commit/3919be4a542fc0d63fcc18422c3465b06fd6c6d1))
* **runtime:** let a file open to read follow its copy up in an Overlay ([10b55f5](https://github.com/elct9620/kobako/commit/10b55f5ba95bfec2de62749917b9ab933836581c))
* **runtime:** let a link leading out escape as a `..` does ([47d7ec7](https://github.com/elct9620/kobako/commit/47d7ec7dfa97bd1d3d1a9bfee0c536372e42108d))
* **runtime:** let a listing hand its own dots and answer for a removed directory ([88a3fc7](https://github.com/elct9620/kobako/commit/88a3fc7e491206b23a3eab2b4c91bf9aa0f00786))
* **runtime:** let a permissive guest tell entries apart by inode number ([e651562](https://github.com/elct9620/kobako/commit/e651562d40fd55028415975f4305f6f3f512ca28))
* **runtime:** let a Policy only refuse more than a Mount's access allows ([f1502b5](https://github.com/elct9620/kobako/commit/f1502b522bcb9d8fc8fffd29a7714b5d777bb793))
* **runtime:** let a Ruby file open report the entry it stands for ([a7bc682](https://github.com/elct9620/kobako/commit/a7bc6821b7d2286079e7b6a1262bfa7a539f3771))
* **runtime:** let a Tmpfs file cut short give its memory back ([ab72b2e](https://github.com/elct9620/kobako/commit/ab72b2ee17cf2f35a12c59b0754233642ee634ea))
* **runtime:** let an entry's setattr decide whether it keeps a time the guest sets ([fdfa3fb](https://github.com/elct9620/kobako/commit/fdfa3fb197909c1f027db5e5c540bc5cbfd5f013))
* **runtime:** let an open file answer for its mode and the largest offset ([a737685](https://github.com/elct9620/kobako/commit/a7376851ba17aa5e9c84a5158424d2ce5d367eaf))
* **runtime:** let atomic_open open a file found under another spelling ([8028d61](https://github.com/elct9620/kobako/commit/8028d616c518eba957aed2d86c51e9c727999f51))
* **runtime:** let go of a deep Tmpfs or dentry in a loop ([13456e3](https://github.com/elct9620/kobako/commit/13456e368580f31a451bfc78be59b631c56569c2))
* **runtime:** let go of the names kept beneath a directory no longer standing ([b1d1468](https://github.com/elct9620/kobako/commit/b1d14682b7afc8c976b0bfe6dd2a1a93ea317091))
* **runtime:** let no Filesystem open follow a link ([09166ac](https://github.com/elct9620/kobako/commit/09166ac45f2b447f9c7e203794163ebed6c991e4))
* **runtime:** let only build_open_flags make an open's flags ([d1ba0ca](https://github.com/elct9620/kobako/commit/d1ba0ca306d59b533abced2eb8927de12afbb85d))
* **runtime:** list a directory as its Filesystem says, through dir_emit ([c7447a1](https://github.com/elct9620/kobako/commit/c7447a137a5237b2e071ba6500fddd736abe5092))
* **runtime:** list a directory from where it stands, as iterate_dir does ([8820933](https://github.com/elct9620/kobako/commit/88209334fc52a162f4bb95a0cd9a5700d5b12459))
* **runtime:** merge only directory over directory in an Overlay ([6473956](https://github.com/elct9620/kobako/commit/6473956fd773a858ea34880b6a1d7b8e60018f3f))
* **runtime:** move a Tmpfs entry's changed time when a name of it changes ([ec97156](https://github.com/elct9620/kobako/commit/ec9715633d319d53d207bf38c2342de9b3ff320d))
* **runtime:** move seeking, flags, hints, allocation and readiness into File ([5d1cd6b](https://github.com/elct9620/kobako/commit/5d1cd6b1d1d626945b5f5c70a5342a8576a017f8))
* **runtime:** name a listing's dots as Linux's drivers do ([ef26d61](https://github.com/elct9620/kobako/commit/ef26d61318ad3f489f5d1379db690854d46a2900))
* **runtime:** name in filesystem every type a Filesystem's operations take ([0cd4a9d](https://github.com/elct9620/kobako/commit/0cd4a9d9c90fd24ad40bd58448c57c8444111e30))
* **runtime:** name the modification time a truncate moves, and set times as one change ([7208428](https://github.com/elct9620/kobako/commit/72084285bdc26eeb840d760ab565ab7c82e65ea3))
* **runtime:** open a Bind's directory for its path alone on Linux ([0de62c1](https://github.com/elct9620/kobako/commit/0de62c10e05ff89d6907d53db5247be6afa6952e))
* **runtime:** open a file for neither reading nor writing as FMODE_PATH ([8827a89](https://github.com/elct9620/kobako/commit/8827a89597955d693b5a8f66e9850c30cdd2da8d))
* **runtime:** open a file made by create alone after the security modules ([5f8d30c](https://github.com/elct9620/kobako/commit/5f8d30c48e08450fa3b1065954fad8ea8e8e5a26))
* **runtime:** open a made file once its directory is unlocked ([1a61088](https://github.com/elct9620/kobako/commit/1a61088b4adbbeaef052e83ad6d8ac19b79d7523))
* **runtime:** open a pseudo file at no position as alloc_file does ([284e15e](https://github.com/elct9620/kobako/commit/284e15e19df1249674d90fd99e0e76f8f61c6c21))
* **runtime:** open a standard stream as a pipe's end, as create_pipe_files does ([58b3f68](https://github.com/elct9620/kobako/commit/58b3f686db485bd39f44e69bca25675acdad215a))
* **runtime:** open as Linux's lookup_open and do_open do ([9109f72](https://github.com/elct9620/kobako/commit/9109f726bb1cab23a9a2729bef8155a2821421e8))
* **runtime:** open each Mount's root before its Policy applies ([4580697](https://github.com/elct9620/kobako/commit/45806973ab0b99f977b7d90253f8dafa9b1f8e8b))
* **runtime:** open only the entry a call judged on a Filesystem reached by path ([6a650e5](https://github.com/elct9620/kobako/commit/6a650e5f92a4ca06234e613b5dac8f3714de5e89))
* **runtime:** read `..` from where a directory stands, and end a walk beneath its start ([f43e34d](https://github.com/elct9620/kobako/commit/f43e34d7f4d80a0b24ced69e50058995148760a0))
* **runtime:** read a call's flags and path before where it starts ([70bce0f](https://github.com/elct9620/kobako/commit/70bce0f0d5b282e1324ab042d5c29b2e523cb0ae))
* **runtime:** read a dentry's parent and name together when asking if it was removed ([4548d8e](https://github.com/elct9620/kobako/commit/4548d8eac4ba5af76e0c5d29d092bf1cea29be0c))
* **runtime:** read a dentry's parent and name together where a Bind asks the host ([d91f7a3](https://github.com/elct9620/kobako/commit/d91f7a3e3f7d8398ddfb13c5a7bfb1537e9722ad))
* **runtime:** read a link at an empty path from the descriptor itself ([c59f6c2](https://github.com/elct9620/kobako/commit/c59f6c2a9c69d40ec08afd961bc6f71ea66cc020))
* **runtime:** read the clock once for a change setting times ([473d32e](https://github.com/elct9620/kobako/commit/473d32ec7ee987bfc30490f39bd031051a924ee2))
* **runtime:** record what a name names in the Filesystem's own lookup, as -&gt;lookup calls d_add ([fab0cfd](https://github.com/elct9620/kobako/commit/fab0cfd24f3eab1d4b52a8a547fda3518644b34d))
* **runtime:** refuse a Bind of a directory the host lets no one read ([6e9730c](https://github.com/elct9620/kobako/commit/6e9730c98dd699109a1cd7eee6fdea2abe1e3b44))
* **runtime:** refuse a change at an Overlay's super block when it has no upper ([7f4c31e](https://github.com/elct9620/kobako/commit/7f4c31e8db91d09fc6a133708e14f28a26d60675))
* **runtime:** refuse a listed name no directory can hold ([e6f4232](https://github.com/elct9620/kobako/commit/e6f4232f3df014b2f49cbff2b9756222d0bdf8ff))
* **runtime:** refuse a name no Layer holds before an Overlay changes anything ([f7ed6da](https://github.com/elct9620/kobako/commit/f7ed6da700c2f8b8c6f91b43421b7721ecd648e4))
* **runtime:** refuse a name Windows reads as another on a Bind ([f8ca1bb](https://github.com/elct9620/kobako/commit/f8ca1bbe6c199cdc39e185ba70229d4976d85822))
* **runtime:** refuse a path no call can take in the VFS ([e752648](https://github.com/elct9620/kobako/commit/e7526485f91ca2acbfe59ad4431d2385c91caa19))
* **runtime:** refuse a seek of a file whose Filesystem names none ([7c827ae](https://github.com/elct9620/kobako/commit/7c827ae78e9797f9ffc36c73e27f31cfc54b8220))
* **runtime:** refuse a spelling a Bind's host lists under no name as EACCES ([bc053d4](https://github.com/elct9620/kobako/commit/bc053d401be0fe409be0dbb00d6ecbef7f33db27))
* **runtime:** refuse a sync of a file without one, as vfs_fsync_range does ([0f26730](https://github.com/elct9620/kobako/commit/0f267304ef9966fb797194817ccfc21134389570))
* **runtime:** refuse an advice from past the largest signed offset ([3ff54a2](https://github.com/elct9620/kobako/commit/3ff54a2f0859c694f012a2ab228295050a500e76))
* **runtime:** refuse one instance as two Layers of an Overlay ([b2c2206](https://github.com/elct9620/kobako/commit/b2c22060f9eceefb2e6a6048bde3960aa4d11dc9))
* **runtime:** refuse removing a path ending in .. as ENOTEMPTY, as Linux does ([a02bac9](https://github.com/elct9620/kobako/commit/a02bac9e9ea300013e90ab396226fe03fbf45f1d))
* **runtime:** refuse to open a directory the host lets no one read ([8dcf10b](https://github.com/elct9620/kobako/commit/8dcf10ba91fdbf196d80ab9dc587eb381091b283))
* **runtime:** refuse to open a link before its Filesystem is asked ([54b0349](https://github.com/elct9620/kobako/commit/54b03493790711056dd051e4492800e444a7d997))
* **runtime:** remove and move through an Overlay's upper as Linux's VFS helpers do ([7973488](https://github.com/elct9620/kobako/commit/79734884adc44c840376b10a0f3dcbec146f6830))
* **runtime:** report the kind a host gives a special file ([b421805](https://github.com/elct9620/kobako/commit/b421805dc4b76cc76f541d1b9e0579329e72a502))
* **runtime:** reserve the room a guest allocates in a Tmpfs and a Bind ([dd94317](https://github.com/elct9620/kobako/commit/dd94317257c3bbbbfd2a2a170a9d5a3783801f43))
* **runtime:** settle a path's last name only after the walk steps into a directory ([7616d08](https://github.com/elct9620/kobako/commit/7616d0800a51f4ac76d39911c9c0ff4c5e83422b))
* **runtime:** size a Tmpfs or Bind directory as Linux does instead of zero ([ed84e7f](https://github.com/elct9620/kobako/commit/ed84e7fdec91e4c10b683b9a75ff2ebc9eaa8c66))
* **runtime:** stand for a path without opening it, as do_o_path ([683383a](https://github.com/elct9620/kobako/commit/683383ac813cf8b716329a6d75b0ce22654f9cfa))
* **runtime:** stop asking where a call starts once moves meet every pass ([6990a1c](https://github.com/elct9620/kobako/commit/6990a1c7a33dd9e9e30a7c83398ebf8ed89e8f5b))
* **runtime:** swap the two dentries an exchange swaps, as Linux does ([b0ca503](https://github.com/elct9620/kobako/commit/b0ca503b74c3d8d659dddb8bfeac471af19e1b89))
* **runtime:** sync a Bind's root and a Ruby Filesystem's directory ([08be1b5](https://github.com/elct9620/kobako/commit/08be1b5975b50e20ca3872649775f92bf617794f))
* **runtime:** sync only as far as asked, cap one call, and let a writer through ([5e2902f](https://github.com/elct9620/kobako/commit/5e2902f95312c985f78053cacf6374db893e174f))
* **runtime:** take a mount's write access in do_dentry_open alone ([32a66a6](https://github.com/elct9620/kobako/commit/32a66a6d97bab6588ea439a84abec01001af3d1a))
* **runtime:** take a name a lookup answered without its directory's lock ([f52ba15](https://github.com/elct9620/kobako/commit/f52ba15f0b151912ab4bf8b9556a67f1a0074d19))
* **runtime:** take a stored spelling's dentry as it stands ([0beebee](https://github.com/elct9620/kobako/commit/0beebee5d12fffeeb9300a75cd5f8e38609e83a4))
* **runtime:** take O_SYNC and O_DIRECTORY's other flags as Linux does ([ba6bc57](https://github.com/elct9620/kobako/commit/ba6bc57e1ad8264e9035d9808098efe5b846f32f))
* **runtime:** walk a dentry's parents under the rename seqlock, as Linux does ([8df90f0](https://github.com/elct9620/kobako/commit/8df90f08bcbf80d59a3c2075bc97f6cad40c69f4))
* **runtime:** walk a path once more where a call is answered ESTALE ([9e4bd0c](https://github.com/elct9620/kobako/commit/9e4bd0cee60e50083ea0e125fa5fdd22187b3c3b))
* **runtime:** walk a path's last name as Linux's namei does ([591435b](https://github.com/elct9620/kobako/commit/591435b5abedf30c62c00fc634bbafe7f1dc68fc))
* **runtime:** walk out of a link and create through one as Linux does ([c6e4054](https://github.com/elct9620/kobako/commit/c6e40547e52421bb7f8a1f86e549c40f365002ad))


### Performance Improvements

* **runtime:** ask a Filesystem once for the name a change makes ([5a490a4](https://github.com/elct9620/kobako/commit/5a490a4c392659842477a2d14a0925dbae066879))
* **runtime:** ask an Overlay's Layers about each name once ([2528559](https://github.com/elct9620/kobako/commit/25285596d8f2a64bca18f16d6a5f7bfaea6e4d0d))
* **runtime:** ask an Overlay's lowest Layer for no markers ([e101335](https://github.com/elct9620/kobako/commit/e101335e53be004359c2851d7723a2bdd9155d5d))
* **runtime:** ask each Layer once for a name an Overlay looks up ([a17f601](https://github.com/elct9620/kobako/commit/a17f601300f52f7d8d6d4d928c9e3fcf682879e0))
* **runtime:** keep names found standing for the next walk ([51dfee4](https://github.com/elct9620/kobako/commit/51dfee488898f130cb0efdb7cb59d1b7a0a38957))
* **runtime:** open the directories a path walks through in one call ([453ad84](https://github.com/elct9620/kobako/commit/453ad84fed5f84b9f16fcf6148482e90c3280844))
* **runtime:** skip the whiteout lookups an Overlay already answered ([69aaefc](https://github.com/elct9620/kobako/commit/69aaefcc74bcf039a65abcbc6c6e113d29098eb1))
* **runtime:** spell an exactly matching Linux name without asking for it ([b499a8c](https://github.com/elct9620/kobako/commit/b499a8cbe145f6e7504fa55a389be181e188763c))
* **runtime:** spell exactly matching Linux names without listing ([3cb8172](https://github.com/elct9620/kobako/commit/3cb81720daa48a90844afad3168070338a43d0e3))
* **runtime:** walk a directory one Overlay Layer shows as that Layer does ([e93dd53](https://github.com/elct9620/kobako/commit/e93dd53a42404d304a94da9b6ea89a5b5f06f009))


### Code Refactoring

* **runtime:** answer a close through flush, as close(2) reads f_op-&gt;flush ([c34eb58](https://github.com/elct9620/kobako/commit/c34eb58b297e5be6ef249af03c6d7655cfd94729))
* **runtime:** answer every VFS call with an errno ([84a0a38](https://github.com/elct9620/kobako/commit/84a0a385149f5a9194fcab9598849cf7c92c46ae))
* **runtime:** drop Mount::filesystem for the mount's own super block ([ac95e32](https://github.com/elct9620/kobako/commit/ac95e32d38372c8e70c57d6656dac4ad3365b708))
* **runtime:** drop the question a Mount no longer has anyone asking ([6cc6249](https://github.com/elct9620/kobako/commit/6cc6249c0b1e47d3aa9f0f86168c991650c38ea2))
* **runtime:** hang each entry's operations on its inode, as Linux does ([e21470b](https://github.com/elct9620/kobako/commit/e21470b625255eb85b4b5ff868872f2db5a7967e))
* **runtime:** land each read and write through a Kiocb, as read_iter and write_iter ([bc35dfd](https://github.com/elct9620/kobako/commit/bc35dfd13085b6aee517748996647bd7083cdbc8))
* **runtime:** let the mount layer own what a Guest may do beneath a Mount ([0dcde21](https://github.com/elct9620/kobako/commit/0dcde2143d7935a8e4b205bb03d664974af7edfb))
* **runtime:** move how a thread waits for a lock beneath the VFS ([616f1c8](https://github.com/elct9620/kobako/commit/616f1c82bd946c3501727e6e5e17d842f5b20ae7))
* **runtime:** name a Filesystem's make-and-open atomic_open ([57a6e10](https://github.com/elct9620/kobako/commit/57a6e10cb14e7a7f6ff02d2a7a88b5e18e459165))
* **runtime:** name what a path reports Kstat, as Linux's struct kstat ([0f43bed](https://github.com/elct9620/kobako/commit/0f43bed4242c2236e02c88d00470d1d31180219e))
* **runtime:** name what an open file answers as Linux's file_operations ([ca835e9](https://github.com/elct9620/kobako/commit/ca835e99b821845bcfa56648ff7336c67242bbd7))
* **runtime:** spell a Kstat's fields as Linux's struct kstat does ([ec73cac](https://github.com/elct9620/kobako/commit/ec73cac38e9b7e8b9162ea59ebc35de7e9394c8d))
* **runtime:** spell each file type as Linux's FT_* names it ([796fac4](https://github.com/elct9620/kobako/commit/796fac412cae9c70b6f80e7f33946fe960ed520e))

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.18.0...kobako-runtime-v0.19.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* **runtime:** a struct literal no longer builds OpenOptions, Metadata, DirEntry, Snapshot, Capture, or Usage outside kobako-runtime; use their `new` or `Default` and assign each field.
* the Guest ABI is version 5, so a Guest Binary built against version 4 is refused at Sandbox construction until rebuilt. In kobako-transport, Reply gains a Break arm and YieldReply::Break carries no payload. The Yielder trait of kobako-runtime gains frame and takes the frame in yield_to_block; the Guest trait of kobako-core and the MrbGuest trait of kobako-mruby take the frame in yield_to_block; Downcall::Yield carries frame and args.
* **runtime:** the Runtime trait's invoke takes the invocation's Grants as a fourth argument, so every engine implementing it must accept them.

### Features

* check what a rename moves as it moves, refusing a directory beneath a Policy ([23794bf](https://github.com/elct9620/kobako/commit/23794bfcb54e148126e29873fa9b52d9e3de8d8c))
* **runtime:** bound the files and directories a Memory or Temporary holds ([8d0be0a](https://github.com/elct9620/kobako/commit/8d0be0ab3dc8c11bcb96865b0bf1a0bbb8dd3b16))
* **runtime:** bound what a Memory or Temporary holds, 64 MiB by default ([2557d1b](https://github.com/elct9620/kobako/commit/2557d1ba7530aacfddf908aeddac561c4fbc5c8a))
* **runtime:** draw every device a host does not name from one source ([210d55b](https://github.com/elct9620/kobako/commit/210d55b1b4e0699dc5ef56b4b6930be01658a3c6))
* **runtime:** hand each invocation its Grants and let an engine declare the kinds it supports ([cba90ee](https://github.com/elct9620/kobako/commit/cba90eed09c8afcb23080cef1c9cbb3650648472)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** keep a directory descriptor's identity its own while open ([6267642](https://github.com/elct9620/kobako/commit/6267642ea831917be76a0101e8d206ccfa8834c1))
* **runtime:** let a dispatch handler abandon its invocation ([179da9e](https://github.com/elct9620/kobako/commit/179da9ee1f31fed9e6d0c149faee0ad55f8db487))
* **runtime:** let a Filesystem's file refuse its close ([af16007](https://github.com/elct9620/kobako/commit/af1600797b098796dd6f329672c7d04e2e1cd2fa)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount decide its own access as the one kind of Grant ([5c9189e](https://github.com/elct9620/kobako/commit/5c9189eea8ac61cf165dca1b38e459c4f14acdd0)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount's Policy judge the spelling the host stores ([5281057](https://github.com/elct9620/kobako/commit/52810573f22d2a3edfae9b34b7767a5f440fba15))
* **runtime:** let the records a Filesystem or an engine builds gain fields ([c0ef88d](https://github.com/elct9620/kobako/commit/c0ef88d0b6df16d651e4c49226e41667307d772f))
* **runtime:** name a file grown past what a Filesystem keeps ([7bac3e6](https://github.com/elct9620/kobako/commit/7bac3e65534097b85d93f5967b6c73a979869a78))
* **runtime:** name each code a Filesystem may fail with ([1d980fa](https://github.com/elct9620/kobako/commit/1d980fa4328bad42e8a8665adfec9fc844690e97))
* **runtime:** name the POSIX error each FilesystemError stands for ([01c301b](https://github.com/elct9620/kobako/commit/01c301b4af9f489594eb7a98ab48c508e5b61ca4))
* **runtime:** refuse a Memory seed path through `..` ([b31434d](https://github.com/elct9620/kobako/commit/b31434d386ebde838c15755619c67da95311ecf6))
* **runtime:** refuse a Mount at a path that is no absolute guest path ([a525758](https://github.com/elct9620/kobako/commit/a5257589f9d831b9a3b6d1952df6e5c8469630e1))
* **runtime:** report a link a lookup does not follow as a link ([8c1e0b3](https://github.com/elct9620/kobako/commit/8c1e0b3e63fbc91b10f11cd91817ce94de24c7e6))
* **runtime:** report the identity of what a path holds ([392bc0e](https://github.com/elct9620/kobako/commit/392bc0e9f2fb935934a0031bf7d64dec0b27df6a))
* **runtime:** say whether a path beneath a Mount may pass a link ([7706a25](https://github.com/elct9620/kobako/commit/7706a2537bf8e5975b2b227bef0c05cb48c09f4b))
* **runtime:** seed a Memory from a list that may not hold, answering why ([8ac2dfd](https://github.com/elct9620/kobako/commit/8ac2dfdf73b84fe2e53405a653d861ddca9ea98c))
* **runtime:** ship the Memory, Temporary, Directory, and Overlay filesystems ([c60c453](https://github.com/elct9620/kobako/commit/c60c4535e966ffd501e03d7be7fef0a44d860b0b)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** stack an Overlay from Layers, each read-only or writable ([6c88188](https://github.com/elct9620/kobako/commit/6c88188cd7992c218f8c5a3be30981e685804078)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Bug Fixes

* append through a Filesystem's file only in one step ([0509ca7](https://github.com/elct9620/kobako/commit/0509ca70d2d5a832303973ba0f7280be0731fd3d))
* ask a mount's block once for a read ([f89bd16](https://github.com/elct9620/kobako/commit/f89bd16c1a71ed1d385ab9d3ac5974ac3f0a9103))
* follow no link swapped in after a Policy judges a path ([ed93213](https://github.com/elct9620/kobako/commit/ed9321303dd45e113fc413e99e1240118cc4bbe4))
* hold an Overlay's lock across each call, waiting without the GVL ([b8e7342](https://github.com/elct9620/kobako/commit/b8e734273fdc577a40be97c43c46d9548d66c625))
* keep a link within the directory a descriptor opened ([952dd05](https://github.com/elct9620/kobako/commit/952dd0556969683f44798da94f9fc7a17c1f6d55))
* run a stored block as the block of the Call that carried it ([bf33dc1](https://github.com/elct9620/kobako/commit/bf33dc1fd40c83498c6c12b0be52b3a835a144cf))
* **runtime:** answer a host failure as the error naming it ([effd43a](https://github.com/elct9620/kobako/commit/effd43a85a3ba69aaa068d2c384c3b5cb466161c))
* **runtime:** append to a Directory file in one host write ([4a5450a](https://github.com/elct9620/kobako/commit/4a5450a67e202e45299175c4fe7528cf50489f0e))
* **runtime:** copy a file up whole when its Layer writes in parts ([89d87f7](https://github.com/elct9620/kobako/commit/89d87f7225611786467968ec34eba5a8223b5fc5))
* **runtime:** fail an Overlay call a Layer cannot answer instead of guessing ([5ba0db3](https://github.com/elct9620/kobako/commit/5ba0db33c56ed837401bcb62345f81b4b50c317a))
* **runtime:** hold each Memory entry under its own name ([8f1ba8a](https://github.com/elct9620/kobako/commit/8f1ba8a50d3d8ca3a695115d51b605be1f18e29e))
* **runtime:** judge a link an Overlay's Layer holds as the link itself ([959c1f3](https://github.com/elct9620/kobako/commit/959c1f3dab6f6948baabbd914bbb86dfff090f7b))
* **runtime:** keep an Overlay rename to one Layer and answer a landed change as done ([e3405b7](https://github.com/elct9620/kobako/commit/e3405b7139809ada202ffdb0d6bf1eef3dcbf056))
* **runtime:** leave a Memory file as it was on a write of no bytes ([9f8fd23](https://github.com/elct9620/kobako/commit/9f8fd237b9b0042590b149f9b2051e0773d8b9af))
* **runtime:** leave an Overlay unchanged when a removal or rename is refused ([a7bee4b](https://github.com/elct9620/kobako/commit/a7bee4b2a465a0ef0ad2e376c281dca82a0df0e1))
* **runtime:** let a file or link in a Layer hide what lies below it ([fd579f6](https://github.com/elct9620/kobako/commit/fd579f679cfa8a83ec7d139066bc1a4570af398b))
* **runtime:** make every Filesystem answer the methods that keep a guarantee ([00f5af6](https://github.com/elct9620/kobako/commit/00f5af63f34ac7722249746d0b0eb566da618803))
* **runtime:** move a directory made again as the writable Layer's own ([ac3e864](https://github.com/elct9620/kobako/commit/ac3e864bbb0ce5d61def45e0403239be405481d3))
* **runtime:** move a directory onto one that holds only markers ([934f0c3](https://github.com/elct9620/kobako/commit/934f0c39e0ad1189bdb1cb0461671245dea32f93))
* **runtime:** open a Directory file the runtime neither reads nor writes ([6b30858](https://github.com/elct9620/kobako/commit/6b308583383d6803894b2c7af1a9ef05d72b0e7d)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** open a file already there without writing to create it ([7f76a1a](https://github.com/elct9620/kobako/commit/7f76a1af575739ae16c5daeb1df8268af44cea63))
* **runtime:** panic on files a Memory cannot be seeded with ([cf5c835](https://github.com/elct9620/kobako/commit/cf5c835d7df8bf82972487280c6a6c79be955a68))
* **runtime:** put an Overlay's markers back when the change fails ([78ddeed](https://github.com/elct9620/kobako/commit/78ddeeddda8b9f797a80ec3796b28e04cf99ffc9))
* **runtime:** refuse a backslash in a Directory path on Windows ([88b5719](https://github.com/elct9620/kobako/commit/88b57195325b168395248a82b94a5ebf7e5f4142))
* **runtime:** refuse a Directory append on Windows rather than take two steps ([a528cc9](https://github.com/elct9620/kobako/commit/a528cc9577f1b2e1359699e2400be93c1f3a9be7))
* **runtime:** refuse a file moved beneath itself as no directory ([bfabb52](https://github.com/elct9620/kobako/commit/bfabb52b27080ee9e5000bd5524ea201a7e33d80))
* **runtime:** refuse a host name that is not UTF-8 as an illegal sequence ([c4ff030](https://github.com/elct9620/kobako/commit/c4ff030ffa8e29fa1b10df4aaf6af65162c76353))
* **runtime:** refuse a Memory file grown past what can be allocated ([302e2d4](https://github.com/elct9620/kobako/commit/302e2d4fcd5b31fee4e953e8993bb47d9a2a673a))
* **runtime:** refuse an Overlay change landing beneath a read-only Layer ([e7b2f19](https://github.com/elct9620/kobako/commit/e7b2f19156df9773f669cbe8a7aaefc17591cca3))
* **runtime:** refuse to open a FIFO, socket, or device beneath a Directory ([bdcaf78](https://github.com/elct9620/kobako/commit/bdcaf78bf131998648c65ac51ced4a588a79211f))
* **runtime:** remove no directory as a file on any host ([c9c0820](https://github.com/elct9620/kobako/commit/c9c0820fd39c0744b9d9c367b977f6587d1f9589))
* **runtime:** rename and copy up through an Overlay as overlayfs does ([ada5a86](https://github.com/elct9620/kobako/commit/ada5a86a8647d89666e9de808eb13ba638882e51))
* **runtime:** replace neither a FIFO nor a directory with the other through an Overlay ([8fed062](https://github.com/elct9620/kobako/commit/8fed062438e105c6a784bf4bde1582fda9e3c9c2))
* **runtime:** report a merged directory's identity alike through an Overlay's lstat ([ded3b4a](https://github.com/elct9620/kobako/commit/ded3b4aba78efa678318c38b139b5c9c76b5d3fd))
* **runtime:** require cap-std 4.0.3 for the trailing-slash lookup fix ([549cfe0](https://github.com/elct9620/kobako/commit/549cfe07fc8b9affdc8a1765a9e4bc5963ef546e))
* **runtime:** reserve an Overlay's marker name in any case ([ef39923](https://github.com/elct9620/kobako/commit/ef399230eabe01ad6eed632b2753880ae1fc322c))
* **runtime:** reserve exactly what a Memory file grows by ([96f8141](https://github.com/elct9620/kobako/commit/96f8141442cac2170a8bf8a76055d4f4659b1100))
* **runtime:** spell a Directory path's last name without following a link there ([c5e5ae7](https://github.com/elct9620/kobako/commit/c5e5ae74ec1f6ee07e24da08d7b097fb6a882961))
* **runtime:** spell a path on XFS by listing rather than trusting exact names ([f89495d](https://github.com/elct9620/kobako/commit/f89495dcfd7c6d05d46ebc5dbae0039856cc15c6))
* **wasmtime:** hold the whole invocation to its deadline ([231e325](https://github.com/elct9620/kobako/commit/231e32592699008adf3105050bee8b5528ea27a8))
* **wasmtime:** place an append at the end as the file stands when it lands ([d7291c4](https://github.com/elct9620/kobako/commit/d7291c4f32fb1f5f2494c69c76796476bbcf4d43))
* **wasmtime:** refuse a link an open asks not to follow ([123a01b](https://github.com/elct9620/kobako/commit/123a01b01f59cc69c0e3a23f9aeb01aa89132c07))


### Performance Improvements

* **runtime:** list an Overlay directory in time linear to its entries ([be3f446](https://github.com/elct9620/kobako/commit/be3f446f782cbd72b114f7b6072e3b50c391f093))
* **runtime:** stop an Overlay's marker lookup at a directory a Layer lacks ([494fa09](https://github.com/elct9620/kobako/commit/494fa090e90806c50e4e8baaa4f1001efc388b45))

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.17.0...kobako-runtime-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **transport:** the stdin layout a Guest Binary reads changed within ABI 4. kobako_transport::envelope::Frames is replaced by Preamble, which is #[non_exhaustive] and built from Default; Bindings and Snippets no longer have their own decode and encode. Guest::eval and Guest::run, MrbGuest::eval and MrbGuest::run, and Runtime::invoke take a Preamble in place of Frames. kobako_core re-exports Preamble and Flag in place of Frames.
* **transport:** the ABI version is 4, so a Guest Binary built for ABI 3 is refused at Sandbox construction. kobako_core::export_guest! emits __kobako_downcall in place of __kobako_eval, __kobako_run, __kobako_yield_to_block and __kobako_take_outcome, and the host import is __kobako_upcall. kobako_core::abi::{take_outcome, write_outcome, write_reply, decode_run} are replaced by serve and answer, and the frames module is no longer public. Reply, YieldReply and Outcome gain an Unsupported arm and take their tags from the shared registry, and kobako_core::DispatchError and kobako::YieldError gain an Unsupported arm. A yield to a guest without block support raises Kobako::UnsupportedError at the Service's yield instead of failing the invocation as a trap.
* **runtime:** Runtime::invoke takes &kobako_transport::envelope::Frames in place of kobako_runtime::Frames, whose byte fields are gone. Guest::eval and Guest::run (and their MrbGuest flows) receive the decoded Frames, eval also its source; kobako_core::frames::read_frame is replaced by read_eval_frames and read_frames.
* **runtime:** kobako_runtime::snapshot::Completion::Outcome holds a decoded kobako_transport::envelope::Outcome instead of Vec<u8>; an engine decodes the result itself and reports an unframed one as
* **runtime:** kobako_runtime::runtime::Entry::Run holds { entrypoint: &str, payload: Vec<u8> } instead of { envelope: &[u8] }; a frontend passes the fields, and an engine encodes the Run envelope itself.
* **runtime:** kobako_runtime::yielder::Yielder::yield_to_block returns Result<YieldReply, Trap> instead of the encoded bytes; an engine implementing it decodes the reply itself.

### Features

* **runtime:** compile a Preamble's snippets on an instance of their own ([303432d](https://github.com/elct9620/kobako/commit/303432dd90ed62dc378391515e68e784e1b473a4)), closes [#75](https://github.com/elct9620/kobako/issues/75)
* **transport:** carry each operation as a tag on one channel per direction ([c7d574b](https://github.com/elct9620/kobako/commit/c7d574b59a42e58632f2bdc46cc9a8e4c7ac6121)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** set an invocation up from a Preamble of tagged records ([84d4498](https://github.com/elct9620/kobako/commit/84d4498464f94d7312f26931ddd4ec43894a6654))


### Bug Fixes

* keep a compile's bytecode only when the Preamble still fits it ([765db08](https://github.com/elct9620/kobako/commit/765db08ad31c8f5163794c7e144355f3237af93b))


### Code Refactoring

* **runtime:** hand a frontend the decoded Outcome ([0da6aac](https://github.com/elct9620/kobako/commit/0da6aac0f7943d694a5a4ab9439b2d6c527ae1b0)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** hand a frontend the decoded Yield Reply ([e80ab54](https://github.com/elct9620/kobako/commit/e80ab54e36c2a80b0f746c675f85bd1238fd4331)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the ABI endpoints frame and read the Frames ([dea069a](https://github.com/elct9620/kobako/commit/dea069ac49a59c12a6f7102ed3cf2925f316cbff)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the engine frame a run's entrypoint and payload ([62f9c94](https://github.com/elct9620/kobako/commit/62f9c9407ff3ce690723625bea075fa49d2dda47)), closes [#73](https://github.com/elct9620/kobako/issues/73)

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.16.0...kobako-runtime-v0.17.0) (2026-09-20)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.15.0...kobako-runtime-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.14.0...kobako-runtime-v0.15.0) (2026-09-13)


### Features

* **spec:** declare what the guest answers when a value will not cross ([886a16f](https://github.com/elct9620/kobako/commit/886a16f173efe44766dd9884ac8bfed61113728a))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.13.1...kobako-runtime-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.13.0...kobako-runtime-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.12.0...kobako-runtime-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **runtime:** mark the sets that grow, and say why the closed ones do not
* **spec:** separate what a name promises from what the wire promises
* **runtime:** give the engine contract names an implementer can write
* **sdk:** attribute an invocation without reading its payload
* **wire:** route every tier through the one envelope
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been ([05b4125](https://github.com/elct9620/kobako/commit/05b41257ca8d4bcb90d6759c6cc7b20582af0661))
* **runtime:** mark the sets that grow, and say why the closed ones do not ([1be2449](https://github.com/elct9620/kobako/commit/1be24492961e2c2a3a317f784cc2f20ae584dcbf))
* **wire:** add the core envelope, implemented independently on both peers ([e6c41d1](https://github.com/elct9620/kobako/commit/e6c41d179e5bf619d43d94160dd597c8ae3bc9bf))
* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))


### Bug Fixes

* **sig:** declare the dispatch seam the shape the ext actually calls ([f0de465](https://github.com/elct9620/kobako/commit/f0de46500eda366d3edd7869edcc39d843f4b687))


### Documentation

* **spec:** separate what a name promises from what the wire promises ([545bfbd](https://github.com/elct9620/kobako/commit/545bfbd59834bab6d564850d9adafa892cbae005))


### Code Refactoring

* **runtime:** give the engine contract names an implementer can write ([0a41491](https://github.com/elct9620/kobako/commit/0a41491360cf17def8245c0758679e92572b990f))
* **sdk:** attribute an invocation without reading its payload ([70d22d0](https://github.com/elct9620/kobako/commit/70d22d01c79034d3d3011e95af5839d6eea654c8))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.11.0...kobako-runtime-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.10.2...kobako-runtime-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.10.1...kobako-runtime-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.10.0...kobako-runtime-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.9.0...kobako-runtime-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.8.0...kobako-runtime-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.7.0...kobako-runtime-v0.8.0) (2026-07-08)


### Bug Fixes

* **crates:** give SetupError a Display so Error::Setup reads cleanly ([bc8b128](https://github.com/elct9620/kobako/commit/bc8b128c9e296319c7fef47441b412d2ce345dff))

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.6.1...kobako-runtime-v0.7.0) (2026-07-03)


### Features

* **crates:** build the requested isolation profile into the WASI context ([63c25d8](https://github.com/elct9620/kobako/commit/63c25d835d4d03010c1658217cee412318e6b5d8))
* **runtime:** runtimes declare their isolation profile ([f89717a](https://github.com/elct9620/kobako/commit/f89717a6bc9809f0de0df78f97da33a49a1474ac))

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.6.0...kobako-runtime-v0.6.1) (2026-07-02)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

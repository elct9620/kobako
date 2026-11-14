:robot: I have created a release *beep* *boop*
---


<details><summary>0.31.0</summary>

## [0.31.0](https://github.com/elct9620/kobako/compare/v0.30.0...v0.31.0) (2026-11-14)


###   BREAKING CHANGES

* **transport:** kobako_transport::abi::MAX_DISPATCH_PAYLOAD is renamed to MAX_MESSAGE_LEN; its value is unchanged.
* **options:** SandboxOptions no longer defines PROFILES or GVL_MODES, and no longer refuses an unknown profile or mode itself; Sandbox.new still refuses both with ArgumentError.
* **options:** SandboxOptions no longer defines the DEFAULT_* constants, and SandboxOptions.new requires every option; a Sandbox's defaults are unchanged and readable through Sandbox#options.
* **runtime:** `memory_limit` now bounds the guest's `memory.grow` and what the host holds for the run together, where a Filesystem written in Ruby had its directory listings counted against a limit of their own. A run that grew close to `memory_limit` and listed a large directory may now raise `Errno::ENOMEM` in the guest or `Kobako::MemoryLimitError`, and `Usage#memory_peak` includes the listings. Raise `memory_limit` by what those listings hold to keep the earlier headroom.
* **sdk:** `RunPayload::bytes`, `RunPayload::build`, and `RunPayload::values` are now `RunPayload::from_bytes`, `RunPayload::from_fn`, and `RunPayload::from_values`, taking the same arguments.
* **runtime:** `kobako_runtime::FilesystemError`, `kobako_runtime::vfs::FilesystemError`, and `kobako_runtime::filesystem::FilesystemError` are now `kobako_runtime::Errno`, also reachable as `kobako::Errno`. Rename the type in a Filesystem or security module; its variants are unchanged.

### Features

* bound a guest's connections per Sandbox, per run, and per process ([7a4f869](https://github.com/elct9620/kobako/commit/7a4f8692cbc788a23c582b90112cd0275739d496))
* **filesystem:** let a Host App mark a Filesystem only the guest changes ([b7e0c81](https://github.com/elct9620/kobako/commit/b7e0c81b7338bd16243a9157de5c8194227b95c8))
* give the guest named network entries through Sandbox#network ([e218017](https://github.com/elct9620/kobako/commit/e218017a77a155ca6ab8c372ec6393a2d1511ada))
* give the GVL up while a guest waits on the host, and end the wait on an interrupt ([bffce05](https://github.com/elct9620/kobako/commit/bffce0569cfcf487f61caeb0dd3964facaffe78d))
* **io:** give a script Addrinfo and its lookups ([e7b9b3c](https://github.com/elct9620/kobako/commit/e7b9b3c2773d4917b11a3c38084317ad1929049b))
* **io:** give a script's TCPSocket setsockopt and getsockopt for SO_KEEPALIVE ([34948a5](https://github.com/elct9620/kobako/commit/34948a529ad8bf141ed5a6862851fc61ea2e53f8))
* **io:** let a script connect with Socket.tcp ([417d63a](https://github.com/elct9620/kobako/commit/417d63ac33e4ba6fe7f43df690d947a5a73714ba))
* **io:** let a script pack socket addresses and read a socket's ends ([89037f9](https://github.com/elct9620/kobako/commit/89037f9ae7f7507bab62f6dc56ecc528fb9d3295))
* **io:** let a script read a socket by byte, by character, and past its buffer ([a0b613a](https://github.com/elct9620/kobako/commit/a0b613acaee61132d4cf109edf28efb414216abc))
* **io:** let a script read and write a socket without waiting ([92145bc](https://github.com/elct9620/kobako/commit/92145bcd51b42d0da1383b9bf436f6eb7b2617f5))
* **io:** let a script set IP_TTL and a UDPSocket's options ([e049750](https://github.com/elct9620/kobako/commit/e0497503dd91e9460fc97f6aaf56cf97b7d89a0f))
* **io:** let a script shut a socket's sides, as CRuby's ([26de1bf](https://github.com/elct9620/kobako/commit/26de1bf311e82958312cf2b43021578e9e20c837))
* **io:** let a script tune TCP keepalive timing and buffer sizes ([662e5cd](https://github.com/elct9620/kobako/commit/662e5cdcd39dd597c99f5752d958f92c15a60eb6))
* **io:** let a script wait on one IO to read or write ([ce0d39c](https://github.com/elct9620/kobako/commit/ce0d39c452f7b2a1fea5bec3d7622094fca1c818))
* **io:** let a script's TCPSocket send through BasicSocket#send ([208a572](https://github.com/elct9620/kobako/commit/208a572b8cdca0270838dca98879aa2261d19f05))
* **io:** make TCPSocket an IO and wait on any IO with IO.select ([d21375b](https://github.com/elct9620/kobako/commit/d21375bbf73a5745e7d5fff57c40dbde5aacd0ad))
* **io:** match patterns and change the working directory through Dir ([72cca2c](https://github.com/elct9620/kobako/commit/72cca2c400c20f6cb52d7fb0e4589fd366bf0878))
* **io:** read a file's lines by its name through File.readlines and File.foreach ([84db4d8](https://github.com/elct9620/kobako/commit/84db4d81064a1f8d03dd1cbca45ba4abf3566bb4))
* **io:** stream a Net::HTTP body and answer CRuby's request helpers ([60bad53](https://github.com/elct9620/kobako/commit/60bad537c7ffe8327de3e3a4d75e198dc183ca85))
* judge a guest's connections with an entry's connect? or a policy(:connect) block ([f9f3c8c](https://github.com/elct9620/kobako/commit/f9f3c8ccdf5571f209a38967ed08ce638f1b4a55))
* judge a guest's lookups with an entry's resolve? or a policy(:resolve) block ([78c2df9](https://github.com/elct9620/kobako/commit/78c2df932dd4023347722e81bfb64e466102cd93))
* let an http endpoint's rules and policy(:request) judge a script's HTTP requests ([9c28d96](https://github.com/elct9620/kobako/commit/9c28d969acdf1b811e4abbf4ba31f845b9013c3f))
* **network:** hand a Sandbox a Mock network from Ruby ([f49ab1d](https://github.com/elct9620/kobako/commit/f49ab1d402dd09fafa189e3c5effa6e88f3966d1))
* open TLS for a script's Net::HTTP with use_ssl, ended on the host ([681f6b8](https://github.com/elct9620/kobako/commit/681f6b8145751a94345c61ed241d143928034406))
* raise NotImplementedError for a TCPSocket in a Sandbox giving no network ([bde27d5](https://github.com/elct9620/kobako/commit/bde27d5763bf56b30fb74b546ac8ba903bb05843))
* reach a loopback server an endpoint names through TCPSocket ([10dbe87](https://github.com/elct9620/kobako/commit/10dbe87ca376b68fa8e30349ccd2dea1b88211b8))
* reach a loopback server an http endpoint names through Net::HTTP ([a88d0a3](https://github.com/elct9620/kobako/commit/a88d0a395bdd8c34c99189fc4e6508f465e10b3b))
* **runtime:** answer the guest's network from a table a test writes ([b3aba65](https://github.com/elct9620/kobako/commit/b3aba650760b8cff3113341500fa414b17f4dd76))
* **runtime:** end a guest's HTTPS on the host, trusting the OS's own store ([5809be2](https://github.com/elct9620/kobako/commit/5809be20fb1127f29811e97062e68cfb11972b82))
* **runtime:** hold a socket's hop limit, and a UDP socket's buffer sizes ([8cf7385](https://github.com/elct9620/kobako/commit/8cf7385474c9ef569a4fee7d025cc5887fe8bb8a))
* **runtime:** hold a TCP socket's keepalive probes and buffer sizes ([a435964](https://github.com/elct9620/kobako/commit/a435964d9d61fcfec7435061b230367008a25fc9))
* **runtime:** hold a TCP socket's keepalive setting for its host socket ([4562f51](https://github.com/elct9620/kobako/commit/4562f510685a92ae02050674968679e15171b637))
* **runtime:** resolve a name to a virtual address the guest connects through ([0dde700](https://github.com/elct9620/kobako/commit/0dde7000c2282d94c90151e8db6b2c9e2c45c837))
* **runtime:** send a guest's HTTP request through an outbound seam dialing only kobako's connector ([43c49fe](https://github.com/elct9620/kobako/commit/43c49fec9b7f7947cb8ed29daf20f3e3d1db8ca1))
* **runtime:** swap an entry's credential into a request bound to its own endpoints ([4b64ccb](https://github.com/elct9620/kobako/commit/4b64ccb61bcae18907bf424d2539052e9588875e))
* swap a #network block's credential into a script's bound HTTPS requests ([370402b](https://github.com/elct9620/kobako/commit/370402b609501837dfd702a8c05182a4a2903bad))
* **transport:** spell once the names a Panic carries that a Frontend reads ([d1fb291](https://github.com/elct9620/kobako/commit/d1fb291082abcc376fc17d2f53b2e7d69dbf5da2))
* **wasmtime:** link the WASI 0.2 sockets, io, and clock imports a core module names ([7c932e7](https://github.com/elct9620/kobako/commit/7c932e770231621ac978bcc43af46877610941c8))


### Bug Fixes

* **core:** take no room for an empty list the host lowers ([f2fac23](https://github.com/elct9620/kobako/commit/f2fac236de645f69e479fa5bff97979005e344eb))
* **ext:** refuse an endpoint object's port out of range as an argument error ([85e7c1d](https://github.com/elct9620/kobako/commit/85e7c1d9d98c0d9876111f823667964c0454be28))
* **ext:** take a duck-typed endpoint's defaults and an IPAddr address ([09a6830](https://github.com/elct9620/kobako/commit/09a6830c43beafc68287e2c9977053468d941126))
* **http:** refuse an outgoing body finished with trailers ([c3c8f1b](https://github.com/elct9620/kobako/commit/c3c8f1b3b6277865f119c04065109a8cb4e1ab76))
* **http:** send the trailers an outgoing body finishes with ([7792bcc](https://github.com/elct9620/kobako/commit/7792bccbedff4758b2ec85bae344c45769997518))
* **io:** answer a directory as its own dot only where CRuby's glob does ([cc79d28](https://github.com/elct9620/kobako/commit/cc79d282c8b45fc0cd2c08a1eb74d5fb81a164b2))
* **io:** answer a script's Net::HTTP edges as CRuby's does ([9818770](https://github.com/elct9620/kobako/commit/98187708a1a003a9b3b32487f6a8a55f1ef171c2))
* **io:** answer a script's socket addr at the guest's own virtual address ([6e8891a](https://github.com/elct9620/kobako/commit/6e8891a9b242849f1d1d9cea825ab47480b69b0e))
* **io:** answer a script's socket edges as CRuby's IO does ([13bc15a](https://github.com/elct9620/kobako/commit/13bc15a324a58afaa139537b7739ead676f718f0))
* **io:** answer a socket's read and write edges as CRuby's ([05521aa](https://github.com/elct9620/kobako/commit/05521aaa5dda7d5f652328768ec2543546285701))
* **io:** answer a UDPSocket send with no peer EDESTADDRREQ before a shut side's EPIPE ([c6cb8c8](https://github.com/elct9620/kobako/commit/c6cb8c8cca6820e2ecdd846ae1981578152f37dc))
* **io:** answer each status with the class CRuby's net/http names ([26e8a52](https://github.com/elct9620/kobako/commit/26e8a529e94057516f035c330573ab3f9eab89c3))
* **io:** answer each stream call with its own error, as CRuby's ([9f54696](https://github.com/elct9620/kobako/commit/9f54696635a4a0026c2555fcc6d0b6ab64b266e4))
* **io:** ask a socket for its options as every other socket call does ([dbdd99a](https://github.com/elct9620/kobako/commit/dbdd99aee80f0f0a7fec7079e65dfec7a83ee2fe))
* **io:** call into the script before any selected socket waits ([7bf9c24](https://github.com/elct9620/kobako/commit/7bf9c24a15c3009197c6820b1bd63fb04b8422cb))
* **io:** check a socket call in the order CRuby's does ([6309c26](https://github.com/elct9620/kobako/commit/6309c261a322dbc19fe6e2c09849a27a5969cd4d))
* **io:** collect every address a name leads to and quote the host as written ([354b276](https://github.com/elct9620/kobako/commit/354b276126c870b9c17a8195ec47566b61db8cef))
* **io:** convert a number a file or socket call takes through to_int ([3da8767](https://github.com/elct9620/kobako/commit/3da8767e53b6c5d759b5a891357c6a3fe50ba01b))
* **io:** convert a send's message, destination, and flags as CRuby's send does ([e0546d6](https://github.com/elct9620/kobako/commit/e0546d6701106eb6950ba3f3609d2e9fb242d96f))
* **io:** convert a socket call's arguments as CRuby's does ([583e84b](https://github.com/elct9620/kobako/commit/583e84bd02f62ff8fdc92843d8576279b4d049c5))
* **io:** drop the carriage return before a chomped newline ([382696f](https://github.com/elct9620/kobako/commit/382696f2227b14169e3b22bca6875e6960e42b3a))
* **io:** hand a Net::HTTP body to a destination as net-http does ([5a6a14b](https://github.com/elct9620/kobako/commit/5a6a14b181d38850f33cf0a836538c529c8f1f01))
* **io:** hold the guest's IO writers to CRuby's ([584781c](https://github.com/elct9620/kobako/commit/584781c75c9608b1448a56f078f5b563fe78d599))
* **io:** judge a UDP send's named peer by its spelling, and advice first ([17aa7c3](https://github.com/elct9620/kobako/commit/17aa7c3c1cac7f53cd04fa25750b75cbb22bce59))
* **io:** keep a socket's number and shut sides in the guest ([0e117f2](https://github.com/elct9620/kobako/commit/0e117f2956dfbc62535e1af8341d36078407e5db))
* **io:** let a UDP socket with no peer wait and keep its shut sides, as Linux's ([04f24de](https://github.com/elct9620/kobako/commit/04f24deed23192982bc271efccf36590bd2abc94))
* **io:** let the host judge a script's bind, as CRuby names it ([0ec0f80](https://github.com/elct9620/kobako/commit/0ec0f805b8a0c6caed0e4c7c25dd99c5cdaffd07))
* **io:** make UDPSocket an IO that IO.select waits on ([1cba17b](https://github.com/elct9620/kobako/commit/1cba17b7f2f7d7da626534bb9a1beed710741503))
* **io:** match a glob pattern by whole characters and its own spelling ([d1699c6](https://github.com/elct9620/kobako/commit/d1699c6d6f5584eb0873baebea358fe4f5cfd326))
* **io:** open a socket with a block as CRuby's IO.open does ([e6938fe](https://github.com/elct9620/kobako/commit/e6938fee87118570aa3027ac8f1257eb14b4d841))
* **io:** raise IOError for a closed UDPSocket's connect and bind before any lookup ([b733892](https://github.com/elct9620/kobako/commit/b73389265149c5a3c87c63eb46e9d3c735b77651))
* **io:** raise IOError for a closed UDPSocket's send before any lookup ([114af24](https://github.com/elct9620/kobako/commit/114af24a67a968befb93314dc8535a3adf455d29))
* **io:** read a host as CRuby's host_str reads it ([ec86710](https://github.com/elct9620/kobako/commit/ec867108bdd872dfcb1d31e23fc93302157c52b7))
* **io:** read a Net::HTTP body into a destination once ([968278f](https://github.com/elct9620/kobako/commit/968278f8c1345e2bed9563835987eaf7482d584d))
* **io:** read a path from what answers to_path or to_str, as CRuby does ([2f9e094](https://github.com/elct9620/kobako/commit/2f9e094a74400fc4b373f22e46d9cf0f433f2c13))
* **io:** read a script's socket and Net::HTTP arguments as CRuby's does ([8b4e3a3](https://github.com/elct9620/kobako/commit/8b4e3a30df09cf8b66930dbbcbf992e903b3777f))
* **io:** read a script's socket through what it holds, as CRuby's IO does ([0d38748](https://github.com/elct9620/kobako/commit/0d387489209580b0d7b33b003e72955141c8da66))
* **io:** read a socket's characters one byte at a time, as CRuby's ([4cdc5cb](https://github.com/elct9620/kobako/commit/4cdc5cb412608798d9dd06bac0d05908ab0d454a))
* **io:** read a socket's port and keywords as CRuby on Linux does ([1df516e](https://github.com/elct9620/kobako/commit/1df516eb94e86a306c450fb5e08700eeae8a2272))
* **io:** read a timeout and exception: only where and as CRuby's does ([480720b](https://github.com/elct9620/kobako/commit/480720b97fc100dd93d7d54bf0333048fa8ab4f9))
* **io:** read a UDPSocket send's flags as CRuby's, refusing one WASI cannot carry ([ac229c4](https://github.com/elct9620/kobako/commit/ac229c48afa104562daefa068ce5a4278123725e))
* **io:** read an address family one way for every socket call ([a2d3c05](https://github.com/elct9620/kobako/commit/a2d3c058cf94348b2d5a8071b66815fdb10e443d))
* **io:** read every timeout as CRuby's rb_time_interval does, in one place ([4cd8158](https://github.com/elct9620/kobako/commit/4cd8158a590d272f89b973ca282cdb0a875c774d))
* **io:** read IO.select's timeout first, and connect_timeout only when a connect waits ([8fe66fa](https://github.com/elct9620/kobako/commit/8fe66fa29d2aeac61c11b964c7327f2737c8e434))
* **io:** read socket and Net::HTTP arguments in CRuby's order ([34bc0c4](https://github.com/elct9620/kobako/commit/34bc0c4887c02410e3e4739ade38b823a7b3646e))
* **io:** read Socket.tcp's resolv_timeout: as the deadline CRuby's takes ([867492a](https://github.com/elct9620/kobako/commit/867492a9c8906a6b641326780da553e50229ec11))
* **io:** receive from a TCPSocket's sender as CRuby's IPSocket does ([ad0b04a](https://github.com/elct9620/kobako/commit/ad0b04aae890a6b3abfce2d5c660e78ea1498b89))
* **io:** refuse an empty path instead of reading the working directory ([1970be2](https://github.com/elct9620/kobako/commit/1970be214fa8d956912fed505d9def61864b7d16))
* **io:** refuse NaN and infinite send flags, and read a UDP destination after its message ([724753f](https://github.com/elct9620/kobako/commit/724753f4faee8d054e00c4eda7f3bc2f559036f8))
* **io:** seek, putc, warn, and an unknown errno answer as CRuby's do ([86f8bff](https://github.com/elct9620/kobako/commit/86f8bffe826a01785edbdf6df142ae9353f30bac))
* **io:** send a script's Net::HTTP request as CRuby's net/http does ([787432d](https://github.com/elct9620/kobako/commit/787432d8d3eebbe7dc02ab12f46571189d267e3d))
* **io:** show an Integer service its port does not spell in Addrinfo#inspect ([b862f5c](https://github.com/elct9620/kobako/commit/b862f5cd981ed6faf3f963ae31d093eef5c80ffb))
* **io:** show and convert an Addrinfo as CRuby's does ([35017c2](https://github.com/elct9620/kobako/commit/35017c2695c5efde3991056188db3d8e52052c2c))
* **io:** shut a script's UDP socket through the socket itself ([bf9b70d](https://github.com/elct9620/kobako/commit/bf9b70d5bf91d4522a543cb28e12fcf60c266d32))
* **io:** take CRuby's TCPSocket arguments and bound Net::HTTP's waits ([892c27f](https://github.com/elct9620/kobako/commit/892c27f9b0bdbebd400535728b90b9da653b6f24))
* **io:** wait on a zero-length receive and name Addrinfo parts as CRuby's ([7bc3ffc](https://github.com/elct9620/kobako/commit/7bc3ffc052fd9b75271b7357b1367fc3994464b4))
* judge a request's authority, path, and headers as wasmtime does ([903cc7b](https://github.com/elct9620/kobako/commit/903cc7b39217f3038de6bab5981ea37499f25e1c))
* **net:** answer EAGAIN on a shut UDP read, as non-blocking Linux does ([675376d](https://github.com/elct9620/kobako/commit/675376d971c721ca23a5bd581e7d83db59dc94aa))
* **net:** answer preview2 permits, options, and schemes as wasmtime-wasi ([3489166](https://github.com/elct9620/kobako/commit/3489166945852ca53626b82510f5c0a728cf0b26))
* **net:** close a guest's TCP socket the way Linux closes one ([c0f95e0](https://github.com/elct9620/kobako/commit/c0f95e0410edd7498bb4a0cea0039cacef1253a4))
* **net:** deny a socket without a network as wasmtime-wasi does ([09b0e9a](https://github.com/elct9620/kobako/commit/09b0e9a2713bd76ab0aafeaa5d459566891154e6))
* **net:** hold a request's parts against memory while it is sent ([d1b642b](https://github.com/elct9620/kobako/commit/d1b642b1171d54b4c0d4cbc1ae6e8ff313b3dc89))
* **net:** hold a request's parts and a response's head against memory ([fbc93c5](https://github.com/elct9620/kobako/commit/fbc93c5d763396cb5b928801380ef3989ec931af))
* **net:** judge a request's method in the case the guest sends it ([d74784a](https://github.com/elct9620/kobako/commit/d74784a464aa60493e3b1718eb9fd5dd47468c8a))
* **net:** judge and reach a connected peer the way the outbound path promises ([8a2797b](https://github.com/elct9620/kobako/commit/8a2797b867c886cf03cd123b1efc3011d7819bac))
* **net:** keep one virtual address per name, and judge a UDP send's peer by it ([8cc7afe](https://github.com/elct9620/kobako/commit/8cc7afe97956a25489bccfa82bf7d1ec7e750818))
* **net:** poll a TCP socket writable once its room is half what it queued ([a1873ee](https://github.com/elct9620/kobako/commit/a1873eead26a9a69f24bfe2d4d17ed9806d758cc))
* **net:** read a host the guest names as wasmtime-wasi's lookup does ([2defe86](https://github.com/elct9620/kobako/commit/2defe8666e9046179dab4ff8b74057e69c9ab10a))
* **net:** refuse a request whose target is not in origin form ([4d02252](https://github.com/elct9620/kobako/commit/4d02252ec6a5c4a714f99cc67f4f79bbab95558a))
* **net:** take a UDP peer's datagrams in before repointing, and refuse an empty label ([a67fa20](https://github.com/elct9620/kobako/commit/a67fa200521fe16379c0db5afb556268a0f606a7))
* **net:** take a UDP socket's datagrams in bounded turns, counting what each holds ([3362df2](https://github.com/elct9620/kobako/commit/3362df24070231ecaa8aec4f556309a02e5b3949))
* **net:** take a UDP socket's turn again once it spends its bound ([2711e0d](https://github.com/elct9620/kobako/commit/2711e0dd45290758bea28031c1cde5c5797ff593))
* **net:** wait on a socket by its own flag, and reset a released one sent to ([9dbac06](https://github.com/elct9620/kobako/commit/9dbac065500e78c84a2ae000f06064f6f21b17f1))
* **net:** wait on a socket whose non-blocking flag was cleared ([ad36e6c](https://github.com/elct9620/kobako/commit/ad36e6c41011f3cebe4f3b653a2bef8c05dbbf21))
* **network:** answer an endpoint the ruleset refuses as the Host App's argument ([1c6d710](https://github.com/elct9620/kobako/commit/1c6d710855495f874761eef1933f51c93a7a62b0))
* **network:** compose a Ruby entry's block through the runtime's Builder ([f62a94a](https://github.com/elct9620/kobako/commit/f62a94a9cc1e95df05fb11e41f51679e48707b11))
* **network:** judge a path rule in work bounded by the path ([20f37fc](https://github.com/elct9620/kobako/commit/20f37fcece42d3aea0018c987948a0814448b38e))
* **network:** read a socket's own end at a virtual port, not the host's ([1ef8486](https://github.com/elct9620/kobako/commit/1ef84864b6f8106e25eaa5fd0abf7310d56835c6))
* **network:** refuse a denied path in another case or with a trailing slash ([ff727bc](https://github.com/elct9620/kobako/commit/ff727bc5307f9739a54878d45e2359868877e4e8))
* **options:** refuse a deadline too far to count instead of aborting ([5611464](https://github.com/elct9620/kobako/commit/561146484100fa67da8d30ce6942b14e87237ab2))
* **parity:** run a scenario naming no cap under each cap's default ([3e306d9](https://github.com/elct9620/kobako/commit/3e306d987f7ca6d56aae1ab5f1ec812664e52ef5))
* refuse a network entry by the native rules alone, where it is written ([49568a6](https://github.com/elct9620/kobako/commit/49568a6cf0f1c45eb08e92c2dfccbfc907634e62))
* **release:** move kobako-codec's kobako-transport pin with each release ([91510e8](https://github.com/elct9620/kobako/commit/91510e8000ecc86afe5518b04b976ca2d77f1508))
* **runtime:** charge empty datagrams and virtual addresses ([9a4d9ba](https://github.com/elct9620/kobako/commit/9a4d9ba92cbd3ba5f7570f1693282202db43b6d9))
* **runtime:** charge the guest's growth and host-held memory to one budget ([f19659e](https://github.com/elct9620/kobako/commit/f19659ed93d0f5b0c1472dbc3bdbbb6815efbb4f))
* **runtime:** check a socket's state before its connect is translated or judged ([044f471](https://github.com/elct9620/kobako/commit/044f471f82aa5ac9edae39fae6dfae31be4e3dbf))
* **runtime:** follow Linux's inet socket through a connect that fails ([596febd](https://github.com/elct9620/kobako/commit/596febdbce08a45f5fc49b713977af5acc664702))
* **runtime:** give a socket its virtual port as Linux's inet layer does ([2c80c02](https://github.com/elct9620/kobako/commit/2c80c027801b5a00501fb8141e8ebde53879a855))
* **runtime:** give back a TCP socket's unnamed port once a connect fails ([03a5d7f](https://github.com/elct9620/kobako/commit/03a5d7fda454ba42a6a6798c1698b282be1c0f4e))
* **runtime:** give each real address its own virtual address and dial one destination ([71c1b8d](https://github.com/elct9620/kobako/commit/71c1b8d7c3ee5ee34f57e5f1a48b43619176d129))
* **runtime:** judge a connect in Linux's order inside the runtime ([f5ede3a](https://github.com/elct9620/kobako/commit/f5ede3af5e8329c21f7630066c1ec539863ef433))
* **runtime:** judge the request target the server reads ([9bc185e](https://github.com/elct9620/kobako/commit/9bc185e6f00375498f4d9119a34357e32d52279b))
* **runtime:** keep a UDP socket's port as it connects again ([070cf8a](https://github.com/elct9620/kobako/commit/070cf8a50c6b8cc93816b5a34711c3910d13333e))
* **runtime:** read a UDP socket's own queue alone, leaving the rest to the reactor's next turn ([b1a9208](https://github.com/elct9620/kobako/commit/b1a92088e56c2f9f934aa4e7f6963f497d30c456))
* **runtime:** refuse a cap of zero in one place both frontends ask ([aef531e](https://github.com/elct9620/kobako/commit/aef531eda4f2b32bc4748b0d09b98058331af3b6))
* **runtime:** refuse a connect when the host's addresses are unread ([47b1cd0](https://github.com/elct9620/kobako/commit/47b1cd0d168a6baa8c94bc5da85513bd3dc7e878))
* **runtime:** refuse a path servers normalize differently ([ba45e4f](https://github.com/elct9620/kobako/commit/ba45e4f5a48bf8f28e4e812465ad122bcbd8f72d))
* **runtime:** take a datagram past the receive buffer into an empty queue ([8f2f670](https://github.com/elct9620/kobako/commit/8f2f670619c7997e0c75a7869e1d5592b4c0e50a))
* **sandbox:** refuse evaluating source whose bytes are not UTF-8 ([83406b7](https://github.com/elct9620/kobako/commit/83406b7dfb9e7b4d09fb68c248469ac9a303a008))
* **sdk:** refuse a run argument carrying a Handle the Host App wrote ([c094a9b](https://github.com/elct9620/kobako/commit/c094a9b137d8090b137f4cd12210ee23d28cfc73))
* **transport:** call a bound BasicObject through Kernel's own methods ([46476bd](https://github.com/elct9620/kobako/commit/46476bd26d3b673b81fd5d24b771e111e90f25f5))
* **transport:** route a Ruby dispatch before reading its arguments ([10c7635](https://github.com/elct9620/kobako/commit/10c76356d106ccd8f2da615075de93170b21a086))
* **vfs:** find a directory's inode in use by its identity ([d817440](https://github.com/elct9620/kobako/commit/d817440b61ca50a192f0f8f1b3d1fd03fe80595d))
* **vfs:** reach a nested mount however the guest spells its path ([73fb96a](https://github.com/elct9620/kobako/commit/73fb96ab63eaf01c73aaef1c803424513b2b07fd))
* **vfs:** stand a nested mount on its mount point's dentry ([9ae2dd8](https://github.com/elct9620/kobako/commit/9ae2dd8b2403bd62faa62208d05a704d00de709a))
* **wasmtime:** answer a connect, a short write, and a datagram's sender as wasmtime-wasi ([d1762d0](https://github.com/elct9620/kobako/commit/d1762d0a6f703c003cdb8bf00c81628f61ce2b6f))
* **wasmtime:** answer a lookup or request past the file limit ([82d9f34](https://github.com/elct9620/kobako/commit/82d9f344a1a322d3b3bb811a8bbf39b4f98ad21f))
* **wasmtime:** answer closed to a blocking flush of nothing once the send side shut ([284d5a0](https://github.com/elct9620/kobako/commit/284d5a0ba7d30ba022d27ba60ed6cc2d4fb0a3c9))
* **wasmtime:** answer three preview2 socket and stream steps as wasmtime-wasi ([702addd](https://github.com/elct9620/kobako/commit/702adddb0a2626117303bcfcc9d80eb4f0bf938e))
* **wasmtime:** answer three socket calls as wasmtime-wasi does ([e0b8c8b](https://github.com/elct9620/kobako/commit/e0b8c8b45d1b2e7b7152b6332c49b5248bd0253d))
* **wasmtime:** ask a UDP address only what wasmtime asks, and its size first ([934d2eb](https://github.com/elct9620/kobako/commit/934d2eb8b7034db263423b8ad925a56ea731b1f4))
* **wasmtime:** bound what one preview2 call lifts by its hostcall fuel ([99ebca7](https://github.com/elct9620/kobako/commit/99ebca726a803d2de0b12bfcbcf7382bf7ab1308))
* **wasmtime:** close a TCP input stream once its receive side shuts ([76d944a](https://github.com/elct9620/kobako/commit/76d944a966e152f9e16e9fc0d61b8765b65a7418))
* **wasmtime:** close an output stream once a call on it fails ([df6b96f](https://github.com/elct9620/kobako/commit/df6b96fc65e3fba8ad4bbf92edebbf35639547d2))
* **wasmtime:** derive preview1's errno from its name, and map two more to preview2 ([aabd599](https://github.com/elct9620/kobako/commit/aabd5997070291dbe5055ae6b075f85f890f674b))
* **wasmtime:** judge a TCP socket's pollable by the connect under way as it is polled ([b89bccd](https://github.com/elct9620/kobako/commit/b89bccdba789d69dc7b69ad07a7176b0d310b963))
* **wasmtime:** make every pollable a child of what it watches ([48a0d2c](https://github.com/elct9620/kobako/commit/48a0d2c1cab25232baf29a9632e9b8a76413d236))
* **wasmtime:** number preview2 handles apart and reach no socket from preview1 ([adb0341](https://github.com/elct9620/kobako/commit/adb03410fd12804ccdaccd94748c6d1415417407))
* **wasmtime:** ready a closed output stream's pollable at once ([4ebd41a](https://github.com/elct9620/kobako/commit/4ebd41a90c5ee96f9e3ca90eea2fcb6246eb6ef5))
* **wasmtime:** refuse a short socket write whole, before any byte leaves ([5be1ec0](https://github.com/elct9620/kobako/commit/5be1ec027474376ecfcadce6d280891efe1a0afa))
* **wasmtime:** round a keepalive time's part of a second up, as wasmtime's reaches the host ([31c5184](https://github.com/elct9620/kobako/commit/31c51843e4e6758179b05725334203915aa42611))
* **wasmtime:** trap a preview2 handle the guest does not hold as it is lifted ([f866c42](https://github.com/elct9620/kobako/commit/f866c4279ab54b581c0e827266c7b9ce75e4c371))
* **wasmtime:** trap on a misaligned preview2 return area ([6192d9e](https://github.com/elct9620/kobako/commit/6192d9e7db3d3f42ab2520b9a3a25d168bd341b6))
* **wasmtime:** wait once per handle a poll lists more than once ([682fb21](https://github.com/elct9620/kobako/commit/682fb21b0623d015ebd5de4b1c78628eba99fef2))
* **wasmtime:** wait through preview2's poll from poll_oneoff ([760a280](https://github.com/elct9620/kobako/commit/760a28052853cbdb2dc9aa49cda2d1883744b86b))


### Code Refactoring

* **options:** judge the profile, the scheduling mode, and the floor once, in the runtime ([4a0eed1](https://github.com/elct9620/kobako/commit/4a0eed18e8ef2e0002e0e61783667084c6fa2e62))
* **options:** take every option's default from the runtime ([1982afe](https://github.com/elct9620/kobako/commit/1982afe89a987a269e1d8c42f69207e05f64ee40))
* **runtime:** name the errno type Errno at the crate root ([f608e31](https://github.com/elct9620/kobako/commit/f608e31e81d1be9ddc6bb2fc8f6a26e5acafced4))
* **sdk:** name RunPayload's constructors with from_ ([6e95532](https://github.com/elct9620/kobako/commit/6e955327f6e9d7c29641f89ac2b1edb58ed24993))
* **transport:** name the message cap by what it bounds ([133e1c1](https://github.com/elct9620/kobako/commit/133e1c151506a1f41d5f2d17ac2678648c6ed262))
</details>

<details><summary>kobako-core: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.20.0...kobako-core-v0.21.0) (2026-11-14)


### Features

* **core:** export cabi_realloc from export_guest! ([dd215f5](https://github.com/elct9620/kobako/commit/dd215f5324457442ad6c031d6c6302698e29b36d))


### Bug Fixes

* **core:** take no room for an empty list the host lowers ([f2fac23](https://github.com/elct9620/kobako/commit/f2fac236de645f69e479fa5bff97979005e344eb))
</details>

<details><summary>kobako-mruby: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.20.0...kobako-mruby-v0.21.0) (2026-11-14)


### Features

* **transport:** spell once the Service and entrypoint names a Panic carries ([fd09028](https://github.com/elct9620/kobako/commit/fd09028dee973397c66eb2029f7fd75bf6e7ee26))
</details>

<details><summary>kobako-io: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.20.0...kobako-io-v0.21.0) (2026-11-14)


### Features

* **io:** give a script Addrinfo and its lookups ([e7b9b3c](https://github.com/elct9620/kobako/commit/e7b9b3c2773d4917b11a3c38084317ad1929049b))
* **io:** give a script's TCPSocket setsockopt and getsockopt for SO_KEEPALIVE ([34948a5](https://github.com/elct9620/kobako/commit/34948a529ad8bf141ed5a6862851fc61ea2e53f8))
* **io:** give the guest UDPSocket over wasi:sockets/udp ([8abaea5](https://github.com/elct9620/kobako/commit/8abaea5b33ffcb24f314f898c4a511d4bbcc708f))
* **io:** import wasi:sockets for TCPSocket ([27d2271](https://github.com/elct9620/kobako/commit/27d227127d184d38fd3f6d8ee9791ebc162af4da))
* **io:** let a script connect with Socket.tcp ([417d63a](https://github.com/elct9620/kobako/commit/417d63ac33e4ba6fe7f43df690d947a5a73714ba))
* **io:** let a script pack socket addresses and read a socket's ends ([89037f9](https://github.com/elct9620/kobako/commit/89037f9ae7f7507bab62f6dc56ecc528fb9d3295))
* **io:** let a script read a socket by byte, by character, and past its buffer ([a0b613a](https://github.com/elct9620/kobako/commit/a0b613acaee61132d4cf109edf28efb414216abc))
* **io:** let a script read and write a socket without waiting ([92145bc](https://github.com/elct9620/kobako/commit/92145bcd51b42d0da1383b9bf436f6eb7b2617f5))
* **io:** let a script set IP_TTL and a UDPSocket's options ([e049750](https://github.com/elct9620/kobako/commit/e0497503dd91e9460fc97f6aaf56cf97b7d89a0f))
* **io:** let a script shut a socket's sides, as CRuby's ([26de1bf](https://github.com/elct9620/kobako/commit/26de1bf311e82958312cf2b43021578e9e20c837))
* **io:** let a script tune TCP keepalive timing and buffer sizes ([662e5cd](https://github.com/elct9620/kobako/commit/662e5cdcd39dd597c99f5752d958f92c15a60eb6))
* **io:** let a script wait on one IO to read or write ([ce0d39c](https://github.com/elct9620/kobako/commit/ce0d39c452f7b2a1fea5bec3d7622094fca1c818))
* **io:** let a script's TCPSocket send through BasicSocket#send ([208a572](https://github.com/elct9620/kobako/commit/208a572b8cdca0270838dca98879aa2261d19f05))
* **io:** make TCPSocket an IO and wait on any IO with IO.select ([d21375b](https://github.com/elct9620/kobako/commit/d21375bbf73a5745e7d5fff57c40dbde5aacd0ad))
* **io:** match patterns and change the working directory through Dir ([72cca2c](https://github.com/elct9620/kobako/commit/72cca2c400c20f6cb52d7fb0e4589fd366bf0878))
* **io:** open a TCPSocket to a name through kobako's resolver ([f0b2fbd](https://github.com/elct9620/kobako/commit/f0b2fbd00ba6f0c9c8320ea785f60a53126fefbd))
* **io:** raise OpenSSL::SSL::SSLError for TLS a request could not open ([c113bbe](https://github.com/elct9620/kobako/commit/c113bbe02b8e1bdfd90c20b4a6dc7bf031ead6fb))
* **io:** read a file's lines by its name through File.readlines and File.foreach ([84db4d8](https://github.com/elct9620/kobako/commit/84db4d81064a1f8d03dd1cbca45ba4abf3566bb4))
* **io:** send and read over TCPSocket through wasi:sockets ([31f86f1](https://github.com/elct9620/kobako/commit/31f86f1e73d7a30e30a7a742ed5de0753293b93a))
* **io:** send HTTP through Net::HTTP over wasi:http ([75747a5](https://github.com/elct9620/kobako/commit/75747a50b866ac090d6f9f8f89fbaca40d48de55))
* **io:** stream a Net::HTTP body and answer CRuby's request helpers ([60bad53](https://github.com/elct9620/kobako/commit/60bad537c7ffe8327de3e3a4d75e198dc183ca85))


### Bug Fixes

* **io:** answer a directory as its own dot only where CRuby's glob does ([cc79d28](https://github.com/elct9620/kobako/commit/cc79d282c8b45fc0cd2c08a1eb74d5fb81a164b2))
* **io:** answer a script's Net::HTTP edges as CRuby's does ([9818770](https://github.com/elct9620/kobako/commit/98187708a1a003a9b3b32487f6a8a55f1ef171c2))
* **io:** answer a script's socket addr at the guest's own virtual address ([6e8891a](https://github.com/elct9620/kobako/commit/6e8891a9b242849f1d1d9cea825ab47480b69b0e))
* **io:** answer a script's socket edges as CRuby's IO does ([13bc15a](https://github.com/elct9620/kobako/commit/13bc15a324a58afaa139537b7739ead676f718f0))
* **io:** answer a socket's read and write edges as CRuby's ([05521aa](https://github.com/elct9620/kobako/commit/05521aaa5dda7d5f652328768ec2543546285701))
* **io:** answer a UDPSocket send with no peer EDESTADDRREQ before a shut side's EPIPE ([c6cb8c8](https://github.com/elct9620/kobako/commit/c6cb8c8cca6820e2ecdd846ae1981578152f37dc))
* **io:** answer each status with the class CRuby's net/http names ([26e8a52](https://github.com/elct9620/kobako/commit/26e8a529e94057516f035c330573ab3f9eab89c3))
* **io:** answer each stream call with its own error, as CRuby's ([9f54696](https://github.com/elct9620/kobako/commit/9f54696635a4a0026c2555fcc6d0b6ab64b266e4))
* **io:** ask a socket for its options as every other socket call does ([dbdd99a](https://github.com/elct9620/kobako/commit/dbdd99aee80f0f0a7fec7079e65dfec7a83ee2fe))
* **io:** call into the script before any selected socket waits ([7bf9c24](https://github.com/elct9620/kobako/commit/7bf9c24a15c3009197c6820b1bd63fb04b8422cb))
* **io:** check a socket call in the order CRuby's does ([6309c26](https://github.com/elct9620/kobako/commit/6309c261a322dbc19fe6e2c09849a27a5969cd4d))
* **io:** collect every address a name leads to and quote the host as written ([354b276](https://github.com/elct9620/kobako/commit/354b276126c870b9c17a8195ec47566b61db8cef))
* **io:** convert a number a file or socket call takes through to_int ([3da8767](https://github.com/elct9620/kobako/commit/3da8767e53b6c5d759b5a891357c6a3fe50ba01b))
* **io:** convert a send's message, destination, and flags as CRuby's send does ([e0546d6](https://github.com/elct9620/kobako/commit/e0546d6701106eb6950ba3f3609d2e9fb242d96f))
* **io:** convert a socket call's arguments as CRuby's does ([583e84b](https://github.com/elct9620/kobako/commit/583e84bd02f62ff8fdc92843d8576279b4d049c5))
* **io:** drop the carriage return before a chomped newline ([382696f](https://github.com/elct9620/kobako/commit/382696f2227b14169e3b22bca6875e6960e42b3a))
* **io:** hand a Net::HTTP body to a destination as net-http does ([5a6a14b](https://github.com/elct9620/kobako/commit/5a6a14b181d38850f33cf0a836538c529c8f1f01))
* **io:** hold the guest's IO writers to CRuby's ([584781c](https://github.com/elct9620/kobako/commit/584781c75c9608b1448a56f078f5b563fe78d599))
* **io:** judge a UDP send's named peer by its spelling, and advice first ([17aa7c3](https://github.com/elct9620/kobako/commit/17aa7c3c1cac7f53cd04fa25750b75cbb22bce59))
* **io:** keep a socket's number and shut sides in the guest ([0e117f2](https://github.com/elct9620/kobako/commit/0e117f2956dfbc62535e1af8341d36078407e5db))
* **io:** let a UDP socket with no peer wait and keep its shut sides, as Linux's ([04f24de](https://github.com/elct9620/kobako/commit/04f24deed23192982bc271efccf36590bd2abc94))
* **io:** let the host judge a script's bind, as CRuby names it ([0ec0f80](https://github.com/elct9620/kobako/commit/0ec0f805b8a0c6caed0e4c7c25dd99c5cdaffd07))
* **io:** make UDPSocket an IO that IO.select waits on ([1cba17b](https://github.com/elct9620/kobako/commit/1cba17b7f2f7d7da626534bb9a1beed710741503))
* **io:** match a glob pattern by whole characters and its own spelling ([d1699c6](https://github.com/elct9620/kobako/commit/d1699c6d6f5584eb0873baebea358fe4f5cfd326))
* **io:** name every errno as CRuby does and build SystemCallError as it does ([487fcb6](https://github.com/elct9620/kobako/commit/487fcb67affc89b445296b6f482cec791f438ccd))
* **io:** name nil, true, and false by value when a String is expected ([b0706ff](https://github.com/elct9620/kobako/commit/b0706ffe61bb39bffe4ff668e6149bac6598685d))
* **io:** open a socket with a block as CRuby's IO.open does ([e6938fe](https://github.com/elct9620/kobako/commit/e6938fee87118570aa3027ac8f1257eb14b4d841))
* **io:** raise IOError for a closed UDPSocket's connect and bind before any lookup ([b733892](https://github.com/elct9620/kobako/commit/b73389265149c5a3c87c63eb46e9d3c735b77651))
* **io:** raise IOError for a closed UDPSocket's send before any lookup ([114af24](https://github.com/elct9620/kobako/commit/114af24a67a968befb93314dc8535a3adf455d29))
* **io:** raise the Errno wasi-libc maps for a socket's in-flight call ([14412d6](https://github.com/elct9620/kobako/commit/14412d6abec939bf220f804ada74f17bf3a41eff))
* **io:** read a file's mode, names, lines, and seeks as CRuby does ([aeb0125](https://github.com/elct9620/kobako/commit/aeb01252708fecab8d5ed968866ca1b368670f0b))
* **io:** read a Float whence as the Integer it truncates to ([5386576](https://github.com/elct9620/kobako/commit/5386576297a76be3c14cc8ce87d1640cab75346e))
* **io:** read a host as CRuby's host_str reads it ([ec86710](https://github.com/elct9620/kobako/commit/ec867108bdd872dfcb1d31e23fc93302157c52b7))
* **io:** read a Net::HTTP body into a destination once ([968278f](https://github.com/elct9620/kobako/commit/968278f8c1345e2bed9563835987eaf7482d584d))
* **io:** read a path from what answers to_path or to_str, as CRuby does ([2f9e094](https://github.com/elct9620/kobako/commit/2f9e094a74400fc4b373f22e46d9cf0f433f2c13))
* **io:** read a script's socket and Net::HTTP arguments as CRuby's does ([8b4e3a3](https://github.com/elct9620/kobako/commit/8b4e3a30df09cf8b66930dbbcbf992e903b3777f))
* **io:** read a script's socket through what it holds, as CRuby's IO does ([0d38748](https://github.com/elct9620/kobako/commit/0d387489209580b0d7b33b003e72955141c8da66))
* **io:** read a socket's characters one byte at a time, as CRuby's ([4cdc5cb](https://github.com/elct9620/kobako/commit/4cdc5cb412608798d9dd06bac0d05908ab0d454a))
* **io:** read a socket's port and keywords as CRuby on Linux does ([1df516e](https://github.com/elct9620/kobako/commit/1df516eb94e86a306c450fb5e08700eeae8a2272))
* **io:** read a timeout and exception: only where and as CRuby's does ([480720b](https://github.com/elct9620/kobako/commit/480720b97fc100dd93d7d54bf0333048fa8ab4f9))
* **io:** read a UDPSocket send's flags as CRuby's, refusing one WASI cannot carry ([ac229c4](https://github.com/elct9620/kobako/commit/ac229c48afa104562daefa068ce5a4278123725e))
* **io:** read an address family one way for every socket call ([a2d3c05](https://github.com/elct9620/kobako/commit/a2d3c058cf94348b2d5a8071b66815fdb10e443d))
* **io:** read every timeout as CRuby's rb_time_interval does, in one place ([4cd8158](https://github.com/elct9620/kobako/commit/4cd8158a590d272f89b973ca282cdb0a875c774d))
* **io:** read IO.select's timeout first, and connect_timeout only when a connect waits ([8fe66fa](https://github.com/elct9620/kobako/commit/8fe66fa29d2aeac61c11b964c7327f2737c8e434))
* **io:** read socket and Net::HTTP arguments in CRuby's order ([34bc0c4](https://github.com/elct9620/kobako/commit/34bc0c4887c02410e3e4739ade38b823a7b3646e))
* **io:** read Socket.tcp's resolv_timeout: as the deadline CRuby's takes ([867492a](https://github.com/elct9620/kobako/commit/867492a9c8906a6b641326780da553e50229ec11))
* **io:** receive from a TCPSocket's sender as CRuby's IPSocket does ([ad0b04a](https://github.com/elct9620/kobako/commit/ad0b04aae890a6b3abfce2d5c660e78ea1498b89))
* **io:** refuse an empty path instead of reading the working directory ([1970be2](https://github.com/elct9620/kobako/commit/1970be214fa8d956912fed505d9def61864b7d16))
* **io:** refuse NaN and infinite send flags, and read a UDP destination after its message ([724753f](https://github.com/elct9620/kobako/commit/724753f4faee8d054e00c4eda7f3bc2f559036f8))
* **io:** seek, putc, warn, and an unknown errno answer as CRuby's do ([86f8bff](https://github.com/elct9620/kobako/commit/86f8bffe826a01785edbdf6df142ae9353f30bac))
* **io:** send a script's Net::HTTP request as CRuby's net/http does ([787432d](https://github.com/elct9620/kobako/commit/787432d8d3eebbe7dc02ab12f46571189d267e3d))
* **io:** show an Integer service its port does not spell in Addrinfo#inspect ([b862f5c](https://github.com/elct9620/kobako/commit/b862f5cd981ed6faf3f963ae31d093eef5c80ffb))
* **io:** show and convert an Addrinfo as CRuby's does ([35017c2](https://github.com/elct9620/kobako/commit/35017c2695c5efde3991056188db3d8e52052c2c))
* **io:** shut a script's UDP socket through the socket itself ([bf9b70d](https://github.com/elct9620/kobako/commit/bf9b70d5bf91d4522a543cb28e12fcf60c266d32))
* **io:** take CRuby's TCPSocket arguments and bound Net::HTTP's waits ([892c27f](https://github.com/elct9620/kobako/commit/892c27f9b0bdbebd400535728b90b9da653b6f24))
* **io:** try each address a name leads to, as CRuby's sockets do ([e808f0a](https://github.com/elct9620/kobako/commit/e808f0a3926881bef820289a86e0a72257c74e22))
* **io:** wait on a zero-length receive and name Addrinfo parts as CRuby's ([7bc3ffc](https://github.com/elct9620/kobako/commit/7bc3ffc052fd9b75271b7357b1367fc3994464b4))
* **net:** answer EAGAIN on a shut UDP read, as non-blocking Linux does ([675376d](https://github.com/elct9620/kobako/commit/675376d971c721ca23a5bd581e7d83db59dc94aa))
* **net:** bound an HTTP request by the timeouts request-options sets ([b0e35d8](https://github.com/elct9620/kobako/commit/b0e35d80a275ecb33679ff1967e05f476f155ce7))
* **net:** deny a socket without a network as wasmtime-wasi does ([09b0e9a](https://github.com/elct9620/kobako/commit/09b0e9a2713bd76ab0aafeaa5d459566891154e6))
* **net:** judge and reach a connected peer the way the outbound path promises ([8a2797b](https://github.com/elct9620/kobako/commit/8a2797b867c886cf03cd123b1efc3011d7819bac))
* **net:** keep one virtual address per name, and judge a UDP send's peer by it ([8cc7afe](https://github.com/elct9620/kobako/commit/8cc7afe97956a25489bccfa82bf7d1ec7e750818))
* **net:** name a host socket's failure from its errno where std names none ([dddffa4](https://github.com/elct9620/kobako/commit/dddffa4bdc22575e3af4b27809662b04127aafc6))
* **runtime:** give each real address its own virtual address and dial one destination ([71c1b8d](https://github.com/elct9620/kobako/commit/71c1b8d7c3ee5ee34f57e5f1a48b43619176d129))
</details>

<details><summary>kobako-regexp: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.20.0...kobako-regexp-v0.21.0) (2026-11-14)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions
</details>

<details><summary>kobako-json: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.20.0...kobako-json-v0.21.0) (2026-11-14)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions
</details>

<details><summary>kobako-baker: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.20.0...kobako-baker-v0.21.0) (2026-11-14)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions
</details>

<details><summary>kobako-transport: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.20.0...kobako-transport-v0.21.0) (2026-11-14)


###   BREAKING CHANGES

* **transport:** kobako_transport::abi::MAX_DISPATCH_PAYLOAD is renamed to MAX_MESSAGE_LEN; its value is unchanged.
* **transport:** ABI_VERSION is 6. A Guest Binary built against version 5 is refused; rebuild it on kobako-core, whose export_guest! now also emits cabi_realloc.

### Features

* **transport:** expose the Capability Handle id bound the Envelope holds ([8a48012](https://github.com/elct9620/kobako/commit/8a48012cbac62c6a4a67c0c47664ee03a8089605))
* **transport:** move the ABI to version 6, exporting cabi_realloc ([2704b30](https://github.com/elct9620/kobako/commit/2704b3036c025e70730c94c35d2097a5ef316738))
* **transport:** spell once the form of a name a guest interns ([95c10bc](https://github.com/elct9620/kobako/commit/95c10bcf7180fc0e5ef432a9935ce3f856c75c9f))
* **transport:** spell once the names a Panic carries that a Frontend reads ([d1fb291](https://github.com/elct9620/kobako/commit/d1fb291082abcc376fc17d2f53b2e7d69dbf5da2))
* **transport:** spell once the Service and entrypoint names a Panic carries ([fd09028](https://github.com/elct9620/kobako/commit/fd09028dee973397c66eb2029f7fd75bf6e7ee26))


### Bug Fixes

* **transport:** refuse a Call target past the wire's Handle id range ([8a9b169](https://github.com/elct9620/kobako/commit/8a9b1694f96dc0769fff3251a76c3e1cb52a4866))


### Code Refactoring

* **transport:** name the message cap by what it bounds ([133e1c1](https://github.com/elct9620/kobako/commit/133e1c151506a1f41d5f2d17ac2678648c6ed262))
</details>

<details><summary>kobako-codec: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.20.0...kobako-codec-v0.21.0) (2026-11-14)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions
</details>

<details><summary>kobako-runtime: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.20.0...kobako-runtime-v0.21.0) (2026-11-14)


###   BREAKING CHANGES

* **sdk:** `kobako::Options` and `kobako_wasmtime::Config` read and set every cap through `limits`: write `options.limits.timeout = &` where `options.timeout = &` was written, and likewise for `setup_timeout`, `memory_limit`, `stdout_limit`, `stderr_limit`, `open_file_limit`, `open_connection_limit`, and `connection_limit`.
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
</details>

<details><summary>kobako-wasmtime: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.20.0...kobako-wasmtime-v0.21.0) (2026-11-14)


###   BREAKING CHANGES

* **sdk:** `kobako::Options` and `kobako_wasmtime::Config` read and set every cap through `limits`: write `options.limits.timeout = &` where `options.timeout = &` was written, and likewise for `setup_timeout`, `memory_limit`, `stdout_limit`, `stderr_limit`, `open_file_limit`, `open_connection_limit`, and `connection_limit`.
* **options:** SandboxOptions no longer defines the DEFAULT_* constants, and SandboxOptions.new requires every option; a Sandbox's defaults are unchanged and readable through Sandbox#options.
* **runtime:** `memory_limit` now bounds the guest's `memory.grow` and what the host holds for the run together, where a Filesystem written in Ruby had its directory listings counted against a limit of their own. A run that grew close to `memory_limit` and listed a large directory may now raise `Errno::ENOMEM` in the guest or `Kobako::MemoryLimitError`, and `Usage#memory_peak` includes the listings. Raise `memory_limit` by what those listings hold to keep the earlier headroom.
* **runtime:** `kobako_runtime::FilesystemError`, `kobako_runtime::vfs::FilesystemError`, and `kobako_runtime::filesystem::FilesystemError` are now `kobako_runtime::Errno`, also reachable as `kobako::Errno`. Rename the type in a Filesystem or security module; its variants are unchanged.
* **runtime:** FileOperations::poll takes a second parameter, Option<&mut PollTable>; write fn poll(&self, mode: FMode, _table: Option<&mut PollTable>) in place of fn poll(&self, mode: FMode). File::poll takes the table too; pass None for a poll that will not sleep.
* **transport:** ABI_VERSION is 6. A Guest Binary built against version 5 is refused; rebuild it on kobako-core, whose export_guest! now also emits cabi_realloc.
* **runtime:** kobako_runtime::mount::Policy moves to kobako_runtime::grant::Policy<R>. Implement Policy<MountRequest<'_>> with allows(&self, request: &MountRequest<'_>) in place of allows(&self, path: &str, access: Access), reading request.path and request.access; a mount_with_policy policy is now Arc<dyn for<'a> Policy<MountRequest<'a>>>, which a closure also fills.

### Features

* bound a guest's connections per Sandbox, per run, and per process ([7a4f869](https://github.com/elct9620/kobako/commit/7a4f8692cbc788a23c582b90112cd0275739d496))
* **io:** match patterns and change the working directory through Dir ([72cca2c](https://github.com/elct9620/kobako/commit/72cca2c400c20f6cb52d7fb0e4589fd366bf0878))
* **runtime:** answer the guest's network from a table a test writes ([b3aba65](https://github.com/elct9620/kobako/commit/b3aba650760b8cff3113341500fa414b17f4dd76))
* **runtime:** let a file's poll register on the poller's wait queue ([05fd5a6](https://github.com/elct9620/kobako/commit/05fd5a626a5ebd21f037e1bbacbf61350a5f5d89))
* **runtime:** send a guest's HTTP request through an outbound seam dialing only kobako's connector ([43c49fe](https://github.com/elct9620/kobako/commit/43c49fec9b7f7947cb8ed29daf20f3e3d1db8ca1))
* **runtime:** wait on wait queues that a producer or an interrupt wakes ([8b15e44](https://github.com/elct9620/kobako/commit/8b15e4484587d3aed5cdf3a88386ffdb99974276))
* **transport:** move the ABI to version 6, exporting cabi_realloc ([2704b30](https://github.com/elct9620/kobako/commit/2704b3036c025e70730c94c35d2097a5ef316738))
* **wasmtime:** answer a TCP socket's keep-alive-enabled from the socket ([0a78137](https://github.com/elct9620/kobako/commit/0a781371651f05196179c2c92cc22f6191e68062))
* **wasmtime:** answer a TCP socket's keepalive timing and buffer sizes ([5b248a3](https://github.com/elct9620/kobako/commit/5b248a3a58c7422c21e8a7bd31cb4f264ab5c3b6))
* **wasmtime:** answer hop limits and a UDP socket's buffer sizes ([3cf0a71](https://github.com/elct9620/kobako/commit/3cf0a71c28acfc2ac4cb7928f3e73b31b924fa90))
* **wasmtime:** answer the client half of wasi:http through the Invocation's namespace ([00844d0](https://github.com/elct9620/kobako/commit/00844d04075e01f436c4933912283deed3048059))
* **wasmtime:** answer wasi:sockets ip-name-lookup through the Invocation's namespace ([de2e84c](https://github.com/elct9620/kobako/commit/de2e84c63e1dbed0931b693c6cde5ec5897bd303))
* **wasmtime:** answer wasi:sockets/udp with one peer a guest names ([e15500d](https://github.com/elct9620/kobako/commit/e15500d6268bb4b31c11367048731a64eaf91577))
* **wasmtime:** connect a guest's TCP socket through the Invocation's namespace ([8656890](https://github.com/elct9620/kobako/commit/8656890d84f561f99351d2bd1e13b2d3f6690562))
* **wasmtime:** connect a guest's TCP socket to an address its endpoints name ([3c982ac](https://github.com/elct9620/kobako/commit/3c982ac52ce58aea2a5af61af5a1a9ed7eccc76a))
* **wasmtime:** count each Driver's socket workers as its Sandbox's ([a046261](https://github.com/elct9620/kobako/commit/a0462613999c9b941eabdfdf4378c152fcfdbba9))
* **wasmtime:** let a frozen clock beside a socket elapse in host time ([b6bd98c](https://github.com/elct9620/kobako/commit/b6bd98c1abbd69bc5b89a276bd7f86b5536bdbd0))
* **wasmtime:** link the WASI 0.2 sockets, io, and clock imports a core module names ([7c932e7](https://github.com/elct9620/kobako/commit/7c932e770231621ac978bcc43af46877610941c8))
* **wasmtime:** read the host's trust store as a Driver is built ([119ede5](https://github.com/elct9620/kobako/commit/119ede5439bb8c12523ef3607a64ee67cd45532d))
* **wasmtime:** take the network's Protocol Family and Resolver from Config ([56cc53f](https://github.com/elct9620/kobako/commit/56cc53fa56cd987666f6b18b16e5264e314d8ef8))


### Bug Fixes

* answer an oversized datagram EMSGSIZE from the socket itself ([02317bc](https://github.com/elct9620/kobako/commit/02317bcc8e1ef86aa4b4a5e7a3b63b297cb6aeab))
* bound a guest's header list by size and charge it to its memory ([80c9ee2](https://github.com/elct9620/kobako/commit/80c9ee2523e0a3ad5de78e769a45136bcbcb6dc8))
* **core:** take no room for an empty list the host lowers ([f2fac23](https://github.com/elct9620/kobako/commit/f2fac236de645f69e479fa5bff97979005e344eb))
* **http:** refuse an outgoing body finished with trailers ([c3c8f1b](https://github.com/elct9620/kobako/commit/c3c8f1b3b6277865f119c04065109a8cb4e1ab76))
* **http:** send the trailers an outgoing body finishes with ([7792bcc](https://github.com/elct9620/kobako/commit/7792bccbedff4758b2ec85bae344c45769997518))
* **io:** answer a script's socket addr at the guest's own virtual address ([6e8891a](https://github.com/elct9620/kobako/commit/6e8891a9b242849f1d1d9cea825ab47480b69b0e))
* **io:** answer each stream call with its own error, as CRuby's ([9f54696](https://github.com/elct9620/kobako/commit/9f54696635a4a0026c2555fcc6d0b6ab64b266e4))
* **io:** convert a number a file or socket call takes through to_int ([3da8767](https://github.com/elct9620/kobako/commit/3da8767e53b6c5d759b5a891357c6a3fe50ba01b))
* **io:** let the host judge a script's bind, as CRuby names it ([0ec0f80](https://github.com/elct9620/kobako/commit/0ec0f805b8a0c6caed0e4c7c25dd99c5cdaffd07))
* **io:** name every errno as CRuby does and build SystemCallError as it does ([487fcb6](https://github.com/elct9620/kobako/commit/487fcb67affc89b445296b6f482cec791f438ccd))
* **io:** read a file's mode, names, lines, and seeks as CRuby does ([aeb0125](https://github.com/elct9620/kobako/commit/aeb01252708fecab8d5ed968866ca1b368670f0b))
* **io:** read a Float whence as the Integer it truncates to ([5386576](https://github.com/elct9620/kobako/commit/5386576297a76be3c14cc8ce87d1640cab75346e))
* **io:** seek, putc, warn, and an unknown errno answer as CRuby's do ([86f8bff](https://github.com/elct9620/kobako/commit/86f8bffe826a01785edbdf6df142ae9353f30bac))
* judge a request's authority, path, and headers as wasmtime does ([903cc7b](https://github.com/elct9620/kobako/commit/903cc7b39217f3038de6bab5981ea37499f25e1c))
* make each guest socket and its file at creation, as socket() does ([fca4311](https://github.com/elct9620/kobako/commit/fca4311060be90df2ec79c1d21bb3a9a5b0b36ca))
* **net:** answer EAGAIN on a shut UDP read, as non-blocking Linux does ([675376d](https://github.com/elct9620/kobako/commit/675376d971c721ca23a5bd581e7d83db59dc94aa))
* **net:** answer preview2 permits, options, and schemes as wasmtime-wasi ([3489166](https://github.com/elct9620/kobako/commit/3489166945852ca53626b82510f5c0a728cf0b26))
* **net:** bound an HTTP request by the timeouts request-options sets ([b0e35d8](https://github.com/elct9620/kobako/commit/b0e35d80a275ecb33679ff1967e05f476f155ce7))
* **net:** deny a socket without a network as wasmtime-wasi does ([09b0e9a](https://github.com/elct9620/kobako/commit/09b0e9a2713bd76ab0aafeaa5d459566891154e6))
* **net:** hold a request's parts against memory while it is sent ([d1b642b](https://github.com/elct9620/kobako/commit/d1b642b1171d54b4c0d4cbc1ae6e8ff313b3dc89))
* **net:** hold a request's parts and a response's head against memory ([fbc93c5](https://github.com/elct9620/kobako/commit/fbc93c5d763396cb5b928801380ef3989ec931af))
* **net:** name a host socket's failure from its errno where std names none ([dddffa4](https://github.com/elct9620/kobako/commit/dddffa4bdc22575e3af4b27809662b04127aafc6))
* **net:** read a host the guest names as wasmtime-wasi's lookup does ([2defe86](https://github.com/elct9620/kobako/commit/2defe8666e9046179dab4ff8b74057e69c9ab10a))
* **net:** start a TCP socket's send buffer at Linux's default until it connects ([ea560c8](https://github.com/elct9620/kobako/commit/ea560c8ecdb0af143f94581ed10147bdc3e0359a))
* **net:** wait on a socket by its own flag, and reset a released one sent to ([9dbac06](https://github.com/elct9620/kobako/commit/9dbac065500e78c84a2ae000f06064f6f21b17f1))
* **network:** answer an endpoint the ruleset refuses as the Host App's argument ([1c6d710](https://github.com/elct9620/kobako/commit/1c6d710855495f874761eef1933f51c93a7a62b0))
* **network:** read a socket's own end at a virtual port, not the host's ([1ef8486](https://github.com/elct9620/kobako/commit/1ef84864b6f8106e25eaa5fd0abf7310d56835c6))
* **runtime:** charge empty datagrams and virtual addresses ([9a4d9ba](https://github.com/elct9620/kobako/commit/9a4d9ba92cbd3ba5f7570f1693282202db43b6d9))
* **runtime:** charge the guest's growth and host-held memory to one budget ([f19659e](https://github.com/elct9620/kobako/commit/f19659ed93d0f5b0c1472dbc3bdbbb6815efbb4f))
* **runtime:** give a socket its virtual port as Linux's inet layer does ([2c80c02](https://github.com/elct9620/kobako/commit/2c80c027801b5a00501fb8141e8ebde53879a855))
* **runtime:** give each Invocation's outbound seam its own TLS sessions ([75119d8](https://github.com/elct9620/kobako/commit/75119d88a49f546a2d827a9476c5067d99bc02dd))
* **runtime:** give each real address its own virtual address and dial one destination ([71c1b8d](https://github.com/elct9620/kobako/commit/71c1b8d7c3ee5ee34f57e5f1a48b43619176d129))
* **runtime:** hold a socket's buffers against the Invocation's memory ([5d629a7](https://github.com/elct9620/kobako/commit/5d629a71f1940222dac36a57502030025575ef70))
* **runtime:** judge the guest's HTTP headers, binds, and peerless streams in the runtime ([bc3e024](https://github.com/elct9620/kobako/commit/bc3e024483e43d8b121cb67e661e06e99997f604))
* **runtime:** judge the request target the server reads ([9bc185e](https://github.com/elct9620/kobako/commit/9bc185e6f00375498f4d9119a34357e32d52279b))
* **runtime:** keep a failed growth in the memory peak ([2e2b986](https://github.com/elct9620/kobako/commit/2e2b986dfa47aff61f0c20ad9b63178bfccbd441))
* **runtime:** let an HTTP exchange go once nothing holds it ([2d78df9](https://github.com/elct9620/kobako/commit/2d78df98d11cdd8671629b1da2a5ac4b0b1862e7))
* **runtime:** refuse a second connect on a connecting or connected socket ([2949d40](https://github.com/elct9620/kobako/commit/2949d402303e11af2b4346a866fee754e1e00dbd))
* **runtime:** take the reactor before a TCP socket dials ([e056522](https://github.com/elct9620/kobako/commit/e0565227c357d05bca12610cd26d4aaac85b604a))
* **wasmtime:** answer a connect, a short write, and a datagram's sender as wasmtime-wasi ([d1762d0](https://github.com/elct9620/kobako/commit/d1762d0a6f703c003cdb8bf00c81628f61ce2b6f))
* **wasmtime:** answer a file's preview1 calls on a socket's number as Linux does ([0ba90a0](https://github.com/elct9620/kobako/commit/0ba90a0b3369065865d6d1b5d3387217f5f37ca2))
* **wasmtime:** answer a lookup or request past the file limit ([82d9f34](https://github.com/elct9620/kobako/commit/82d9f344a1a322d3b3bb811a8bbf39b4f98ad21f))
* **wasmtime:** answer a socket's edges as wasmtime-wasi does ([2b80a61](https://github.com/elct9620/kobako/commit/2b80a61876c9eef18d8100df46b05033825b5b58))
* **wasmtime:** answer closed to a blocking flush of nothing once the send side shut ([284d5a0](https://github.com/elct9620/kobako/commit/284d5a0ba7d30ba022d27ba60ed6cc2d4fb0a3c9))
* **wasmtime:** answer new-socket-limit for a socket past the open file limit ([e45c8e8](https://github.com/elct9620/kobako/commit/e45c8e8a86003c47f69e251f0e87a77764ed2fa9))
* **wasmtime:** answer preview1 reads, writes, and polls on a socket's number ([5514429](https://github.com/elct9620/kobako/commit/5514429067d83e6f5775bff21a1f8ac09b70eff6))
* **wasmtime:** answer the non-blocking stream calls a guest bounds its waits with ([16ef908](https://github.com/elct9620/kobako/commit/16ef908947f5c94a37ee0f5370038936b2052e4a))
* **wasmtime:** answer the socket and stream edges as wasmtime answers them ([4f690ea](https://github.com/elct9620/kobako/commit/4f690ea29f45451982dee726c42dd178dc4ae880))
* **wasmtime:** answer three preview2 socket and stream steps as wasmtime-wasi ([702addd](https://github.com/elct9620/kobako/commit/702adddb0a2626117303bcfcc9d80eb4f0bf938e))
* **wasmtime:** answer three socket calls as wasmtime-wasi does ([e0b8c8b](https://github.com/elct9620/kobako/commit/e0b8c8b45d1b2e7b7152b6332c49b5248bd0253d))
* **wasmtime:** ask a UDP address only what wasmtime asks, and its size first ([934d2eb](https://github.com/elct9620/kobako/commit/934d2eb8b7034db263423b8ad925a56ea731b1f4))
* **wasmtime:** bound what one preview2 call lifts by its hostcall fuel ([99ebca7](https://github.com/elct9620/kobako/commit/99ebca726a803d2de0b12bfcbcf7382bf7ab1308))
* **wasmtime:** check a preview2 return area whole once the call returns ([9910dea](https://github.com/elct9620/kobako/commit/9910dea5e4e13056d9007b78d99f67486ebb0bdc))
* **wasmtime:** close a TCP input stream once its receive side shuts ([76d944a](https://github.com/elct9620/kobako/commit/76d944a966e152f9e16e9fc0d61b8765b65a7418))
* **wasmtime:** close an output stream once a call on it fails ([df6b96f](https://github.com/elct9620/kobako/commit/df6b96fc65e3fba8ad4bbf92edebbf35639547d2))
* **wasmtime:** count a socket, a lookup, or an exchange as one file ([b5633e4](https://github.com/elct9620/kobako/commit/b5633e4c1e2d1efba35ef4a9c3f7f97b2c1ea65e))
* **wasmtime:** derive preview1's errno from its name, and map two more to preview2 ([aabd599](https://github.com/elct9620/kobako/commit/aabd5997070291dbe5055ae6b075f85f890f674b))
* **wasmtime:** end a run its host gave up at its next preview2 call ([0d1b324](https://github.com/elct9620/kobako/commit/0d1b32494c501ee346f50e39e8e38d7fb885dab9))
* **wasmtime:** give back the charge of a growth wasmtime then fails ([070c463](https://github.com/elct9620/kobako/commit/070c463e11428eb27465d6d9a9e7ba38b738249f))
* **wasmtime:** hold a socket's streams and pollables as its children ([0957eda](https://github.com/elct9620/kobako/commit/0957eda5a2159e402573ddfa7e2de5e941adea80))
* **wasmtime:** judge a TCP socket's pollable by the connect under way as it is polled ([b89bccd](https://github.com/elct9620/kobako/commit/b89bccdba789d69dc7b69ad07a7176b0d310b963))
* **wasmtime:** make every pollable a child of what it watches ([48a0d2c](https://github.com/elct9620/kobako/commit/48a0d2c1cab25232baf29a9632e9b8a76413d236))
* **wasmtime:** number preview2 handles apart and reach no socket from preview1 ([adb0341](https://github.com/elct9620/kobako/commit/adb03410fd12804ccdaccd94748c6d1415417407))
* **wasmtime:** number preview2 handles from the descriptor space and trap a misnamed drop ([925a724](https://github.com/elct9620/kobako/commit/925a7244007e7fd72bec43f1b5dd5fa0d419dc84))
* **wasmtime:** permit a datagram send whenever its pollable is ready, and end a full flush on close ([8cf4961](https://github.com/elct9620/kobako/commit/8cf49612ee3af7ae5002ffa7b373a8b0579c2b78))
* **wasmtime:** poll an output stream ready only when it takes a write ([3247e0e](https://github.com/elct9620/kobako/commit/3247e0e80e5fee98e83e562b8fc99484d5863b8e))
* **wasmtime:** ready a closed output stream's pollable at once ([4ebd41a](https://github.com/elct9620/kobako/commit/4ebd41a90c5ee96f9e3ca90eea2fcb6246eb6ef5))
* **wasmtime:** refuse a short socket write whole, before any byte leaves ([5be1ec0](https://github.com/elct9620/kobako/commit/5be1ec027474376ecfcadce6d280891efe1a0afa))
* **wasmtime:** report RSYNC beside SYNC for a file opened to sync ([da46930](https://github.com/elct9620/kobako/commit/da46930be0a874bbdce2c6bccb85355a769b4912))
* **wasmtime:** round a keepalive time's part of a second up, as wasmtime's reaches the host ([31c5184](https://github.com/elct9620/kobako/commit/31c51843e4e6758179b05725334203915aa42611))
* **wasmtime:** trap a preview2 handle the guest does not hold as it is lifted ([f866c42](https://github.com/elct9620/kobako/commit/f866c4279ab54b581c0e827266c7b9ce75e4c371))
* **wasmtime:** trap and fail where the WIT says, and map errors as wasmtime-wasi does ([f8241ba](https://github.com/elct9620/kobako/commit/f8241baa1ebe9e399e6ce658f60d24e716b59522))
* **wasmtime:** trap on a misaligned preview2 return area ([6192d9e](https://github.com/elct9620/kobako/commit/6192d9e7db3d3f42ab2520b9a3a25d168bd341b6))
* **wasmtime:** trap on a preview2 list past the guest's memory before reading it ([7d9dc3c](https://github.com/elct9620/kobako/commit/7d9dc3ce63d8936053046cacaef4b31a6dadf86a))
* **wasmtime:** wait once per handle a poll lists more than once ([682fb21](https://github.com/elct9620/kobako/commit/682fb21b0623d015ebd5de4b1c78628eba99fef2))
* **wasmtime:** wait through preview2's poll from poll_oneoff ([760a280](https://github.com/elct9620/kobako/commit/760a28052853cbdb2dc9aa49cda2d1883744b86b))


### Code Refactoring

* **options:** take every option's default from the runtime ([1982afe](https://github.com/elct9620/kobako/commit/1982afe89a987a269e1d8c42f69207e05f64ee40))
* **runtime:** judge each Grant's requests through one generic Policy ([770fc66](https://github.com/elct9620/kobako/commit/770fc665b00532f4092dfdb8a864e02d387065c3))
* **runtime:** name the errno type Errno at the crate root ([f608e31](https://github.com/elct9620/kobako/commit/f608e31e81d1be9ddc6bb2fc8f6a26e5acafced4))
* **sdk:** hold a Sandbox's caps as the runtime's Limits ([9730d6a](https://github.com/elct9620/kobako/commit/9730d6a73fec595c2b75ac8981aeb655937045df))
</details>

<details><summary>kobako-sdk: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.20.0...kobako-sdk-v0.21.0) (2026-11-14)


###   BREAKING CHANGES

* **sdk:** `kobako::Options` and `kobako_wasmtime::Config` read and set every cap through `limits`: write `options.limits.timeout = &` where `options.timeout = &` was written, and likewise for `setup_timeout`, `memory_limit`, `stdout_limit`, `stderr_limit`, `open_file_limit`, `open_connection_limit`, and `connection_limit`.
* **sdk:** `RunPayload::bytes`, `RunPayload::build`, and `RunPayload::values` are now `RunPayload::from_bytes`, `RunPayload::from_fn`, and `RunPayload::from_values`, taking the same arguments.
* **runtime:** `kobako_runtime::FilesystemError`, `kobako_runtime::vfs::FilesystemError`, and `kobako_runtime::filesystem::FilesystemError` are now `kobako_runtime::Errno`, also reachable as `kobako::Errno`. Rename the type in a Filesystem or security module; its variants are unchanged.
* **runtime:** kobako_runtime::mount::Policy moves to kobako_runtime::grant::Policy<R>. Implement Policy<MountRequest<'_>> with allows(&self, request: &MountRequest<'_>) in place of allows(&self, path: &str, access: Access), reading request.path and request.access; a mount_with_policy policy is now Arc<dyn for<'a> Policy<MountRequest<'a>>>, which a closure also fills.

### Features

* bound a guest's connections per Sandbox, per run, and per process ([7a4f869](https://github.com/elct9620/kobako/commit/7a4f8692cbc788a23c582b90112cd0275739d496))
* **runtime:** give the guest named network entries as a Grant ([85cfc10](https://github.com/elct9620/kobako/commit/85cfc10b029c2a94adc45d1809fbbd7c651d94f9))
* **sdk:** add Sandbox::network_with to build on an entry in a closure ([9e050df](https://github.com/elct9620/kobako/commit/9e050df718126f389896b0b2253eebc2bd882139))
* **sdk:** take the network's backends from Options ([5b5b5d2](https://github.com/elct9620/kobako/commit/5b5b5d2b437eed5df83c8476ba4ff5eea178ea9f))
* **transport:** spell once the form of a name a guest interns ([95c10bc](https://github.com/elct9620/kobako/commit/95c10bcf7180fc0e5ef432a9935ce3f856c75c9f))


### Bug Fixes

* **network:** answer an endpoint the ruleset refuses as the Host App's argument ([1c6d710](https://github.com/elct9620/kobako/commit/1c6d710855495f874761eef1933f51c93a7a62b0))
* **runtime:** refuse a cap of zero in one place both frontends ask ([aef531e](https://github.com/elct9620/kobako/commit/aef531eda4f2b32bc4748b0d09b98058331af3b6))
* **sdk:** refuse a run argument carrying a Handle the Host App wrote ([c094a9b](https://github.com/elct9620/kobako/commit/c094a9b137d8090b137f4cd12210ee23d28cfc73))
* **transport:** route a Ruby dispatch before reading its arguments ([10c7635](https://github.com/elct9620/kobako/commit/10c76356d106ccd8f2da615075de93170b21a086))


### Code Refactoring

* **runtime:** judge each Grant's requests through one generic Policy ([770fc66](https://github.com/elct9620/kobako/commit/770fc665b00532f4092dfdb8a864e02d387065c3))
* **runtime:** name the errno type Errno at the crate root ([f608e31](https://github.com/elct9620/kobako/commit/f608e31e81d1be9ddc6bb2fc8f6a26e5acafced4))
* **sdk:** hold a Sandbox's caps as the runtime's Limits ([9730d6a](https://github.com/elct9620/kobako/commit/9730d6a73fec595c2b75ac8981aeb655937045df))
* **sdk:** name RunPayload's constructors with from_ ([6e95532](https://github.com/elct9620/kobako/commit/6e955327f6e9d7c29641f89ac2b1edb58ed24993))
</details>

---
This PR was generated with [Release Please](https://github.com/googleapis/release-please). See [documentation](https://github.com/googleapis/release-please#release-please).
:robot: I have created a release *beep* *boop*
---


<details><summary>0.31.0</summary>

## [0.31.0](https://github.com/elct9620/kobako/compare/v0.30.0...v0.31.0) (2026-11-18)


###   BREAKING CHANGES

* **transport:** kobako_transport::envelope::Reply is CallReply.
* **guest:** call kobako_core::abi::handle and return_reply in place of serve and answer.
* **guest:** call Kobako::exc_backtrace in place of
* **transport:** call Driver::from_file in place of Driver::new;
* **guest:** call Kobako::handle_id_of, Kobako::backtrace_of,
* **security:** call kobako_runtime::security::prepare_creds in place of prepare_creds_within.
* **vfs:** call Rdir::iterate_shared and File::is_readable, is_writable, is_append, is_nonblock, is_dsync, and is_sync in place of the bare names.
* **vfs:** call Iattr::for_size, Iattr::for_times, Kiocb::new, and get_anon_bdev in place of Iattr::size, Iattr::times, Kiocb::at, and
* **vfs:** call Inode::file_type and File::file_type in place of kind.
* **runtime:** match SetupError::GuestBinaryNotFound in place of
* **errors:** rescue Kobako::GuestBinaryNotFoundError in place of
* **sdk:** write Backend { path, lifetime: Lifetime::Static(..) } in place of Backend { path, provider: Provider::Static(..) }.
* **errors:** read SandboxError#class_name, ServiceError#class_name, and BlockError#class_name in place of #klass.
* **pool:** Kobako::Pool.new takes size: in place of slots:.
* **network:** open_connection_limit bounds what one invocation holds open at once, not what all of a Sandbox's invocations hold together.
* **sdk:** Sandbox::mount_with_policy and Context::mount_with_policy take (at, filesystem, access, policy); pass Access::ReadWrite for the previous read-write mount.
* **sdk:** an implementation of Receiver or ValueReceiver overrides responds_to_guest in place of respond_to_guest.
* **network:** an Entry's Policies are built with with_request_policy, with_resolve_policy, and with_connect_policy in place of with_request, with_resolve, and with_connect.
* **network:** Kobako::Network::Endpoint, HttpRule, Credential, and an entry's endpoints raise TypeError for text neither a Symbol nor a String, and Sandbox#install raises TypeError for a depends_on that is not an Array of them, where each was accepted through to_s or failed later.
* **sandbox:** Sandbox#run raises TypeError for a Hash key or a reflective object it cannot carry, and ArgumentError for an argument nested past the wire's depth, where it raised Kobako::SandboxError; a rescue of Kobako::SandboxError around #run no longer catches them.
* **sandbox:** Sandbox#bind and Context#bind given a path that is no Symbol or String raise TypeError instead of ArgumentError, and Sandbox#network and Context#network given such a name raise TypeError instead of registering it under its to_s. Pass the name as a Symbol or String.
* **kobako:** Error::Argument is a struct variant, Error::Argument { kind, message }, instead of Error::Argument(String). Match Error::Argument { message, .. } where the message was matched, and Error::Argument { kind: ArgumentKind::Taken, .. } to tell one mistake from another; a match on ArgumentKind keeps a wildcard arm.
* **sandbox:** a bind, mount, network, preload, flag, or install after the first invocation raises FrozenError instead of ArgumentError; an argument of the wrong class to preload, flag, install, a run's keyword keys, Sandbox.new's caps, or Pool.new raises TypeError instead of ArgumentError; a cap Integer past what the host counts, or a deadline past what it times, raises RangeError; Sandbox#eval raises TypeError or ArgumentError instead of Kobako::SandboxError for source it refuses. Rescue the new class, or StandardError to cover both versions.
* **transport:** a Guest Binary built against ABI 5 is refused at construction. Rebuild it against kobako-core, whose export_guest! emits the new names. kobako-transport drops ABI_VERSION as a u32, FRAME_LEN_SIZE, MAX_FRAME_LEN, pack_ptr_len, and unpack_ptr_len, and names the ABI through ABI_VERSION ("0.6.0"), GUEST_INTERFACE, HOST_INTERFACE, DOWNCALL, UPCALL, and PREAMBLE. kobako-core drops alloc; answer returns the return area's address.
* **transport:** kobako_transport::abi::MAX_DISPATCH_PAYLOAD is renamed to MAX_MESSAGE_LEN; its value is unchanged.
* **options:** SandboxOptions no longer defines PROFILES or GVL_MODES, and no longer refuses an unknown profile or mode itself; Sandbox.new still refuses both with ArgumentError.
* **options:** SandboxOptions no longer defines the DEFAULT_* constants, and SandboxOptions.new requires every option; a Sandbox's defaults are unchanged and readable through Sandbox#options.
* **runtime:** `memory_limit` now bounds the guest's `memory.grow` and what the host holds for the run together, where a Filesystem written in Ruby had its directory listings counted against a limit of their own. A run that grew close to `memory_limit` and listed a large directory may now raise `Errno::ENOMEM` in the guest or `Kobako::MemoryLimitError`, and `Usage#memory_peak` includes the listings. Raise `memory_limit` by what those listings hold to keep the earlier headroom.
* **sdk:** `RunPayload::bytes`, `RunPayload::build`, and `RunPayload::values` are now `RunPayload::from_bytes`, `RunPayload::from_fn`, and `RunPayload::from_values`, taking the same arguments.
* **runtime:** `kobako_runtime::FilesystemError`, `kobako_runtime::vfs::FilesystemError`, and `kobako_runtime::filesystem::FilesystemError` are now `kobako_runtime::Errno`, also reachable as `kobako::Errno`. Rename the type in a Filesystem or security module; its variants are unchanged.

### Features

* bound a guest's connections per Sandbox, per run, and per process ([7a4f869](https://github.com/elct9620/kobako/commit/7a4f8692cbc788a23c582b90112cd0275739d496))
* **filesystem:** let a Host App mark a Filesystem only the guest changes ([b7e0c81](https://github.com/elct9620/kobako/commit/b7e0c81b7338bd16243a9157de5c8194227b95c8))
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
* **io:** name the socket constants CRuby does, and wait as IO#wait does ([fb758d7](https://github.com/elct9620/kobako/commit/fb758d7d3cf950f2cb69c6daa1b3a235e6352e6e))
* **io:** read a file's lines by its name through File.readlines and File.foreach ([84db4d8](https://github.com/elct9620/kobako/commit/84db4d81064a1f8d03dd1cbca45ba4abf3566bb4))
* **io:** stream a Net::HTTP body and answer CRuby's request helpers ([60bad53](https://github.com/elct9620/kobako/commit/60bad537c7ffe8327de3e3a4d75e198dc183ca85))
* **kobako:** name which mistake an Error::Argument refused ([43dfd57](https://github.com/elct9620/kobako/commit/43dfd5711a8250075524fa9f1fc003102e928c76))
* let an http endpoint's rules and policy(:request) judge a script's HTTP requests ([9c28d96](https://github.com/elct9620/kobako/commit/9c28d969acdf1b811e4abbf4ba31f845b9013c3f))
* **network:** hand a Sandbox a Mock network from Ruby ([f49ab1d](https://github.com/elct9620/kobako/commit/f49ab1d402dd09fafa189e3c5effa6e88f3966d1))
* **network:** route and refuse a Mock's dials by protocol, UDP included ([9e8b44d](https://github.com/elct9620/kobako/commit/9e8b44d425db205fe0bb2faccca0782acf1e1b4e))
* open TLS for a script's Net::HTTP with use_ssl, ended on the host ([681f6b8](https://github.com/elct9620/kobako/commit/681f6b8145751a94345c61ed241d143928034406))
* **pool:** deprecate Kobako::Pool ahead of its removal ([947b8e8](https://github.com/elct9620/kobako/commit/947b8e8a80c4eca19696513672c7171c5ba21069))
* reach a loopback server an http endpoint names through Net::HTTP ([a88d0a3](https://github.com/elct9620/kobako/commit/a88d0a395bdd8c34c99189fc4e6508f465e10b3b))
* **runtime:** answer the guest's network from a table a test writes ([b3aba65](https://github.com/elct9620/kobako/commit/b3aba650760b8cff3113341500fa414b17f4dd76))
* **runtime:** end a guest's HTTPS on the host, trusting the OS's own store ([5809be2](https://github.com/elct9620/kobako/commit/5809be20fb1127f29811e97062e68cfb11972b82))
* **runtime:** hold a socket's hop limit, and a UDP socket's buffer sizes ([8cf7385](https://github.com/elct9620/kobako/commit/8cf7385474c9ef569a4fee7d025cc5887fe8bb8a))
* **runtime:** hold a TCP socket's keepalive probes and buffer sizes ([a435964](https://github.com/elct9620/kobako/commit/a435964d9d61fcfec7435061b230367008a25fc9))
* **runtime:** hold a TCP socket's keepalive setting for its host socket ([4562f51](https://github.com/elct9620/kobako/commit/4562f510685a92ae02050674968679e15171b637))
* **runtime:** make a Filesystem from its kind without wrapping it first ([e7eb8a0](https://github.com/elct9620/kobako/commit/e7eb8a00a37794226edce2aa7f1bdcab2ef6790b))
* **runtime:** swap an entry's credential into a request bound to its own endpoints ([4b64ccb](https://github.com/elct9620/kobako/commit/4b64ccb61bcae18907bf424d2539052e9588875e))
* **sandbox:** warn once when a run releasing the GVL runs on the main Thread ([9be6583](https://github.com/elct9620/kobako/commit/9be658368a72812017bd3f883eec94f0b14a3cd0))
* **sdk:** let a Rust mount with a policy keep the access it is given ([3d10278](https://github.com/elct9620/kobako/commit/3d102786f481d1e832c262b05e62616da4f8be28))
* swap a #network block's credential into a script's bound HTTPS requests ([370402b](https://github.com/elct9620/kobako/commit/370402b609501837dfd702a8c05182a4a2903bad))
* **transport:** carry the Guest ABI in the Canonical ABI's core shape ([34b9560](https://github.com/elct9620/kobako/commit/34b956001a05e15fe29febd7098b2ceb054e1bab))
* **transport:** spell once the names a Panic carries that a Frontend reads ([d1fb291](https://github.com/elct9620/kobako/commit/d1fb291082abcc376fc17d2f53b2e7d69dbf5da2))
* **wasmtime:** stand the standard streams over preview2 streams ([768ba2b](https://github.com/elct9620/kobako/commit/768ba2b608e09915b3399108ffcca0d163004177))


### Bug Fixes

* **codec:** charge a map's NaN keys for the comparisons they cost a host ([62eead4](https://github.com/elct9620/kobako/commit/62eead43663705a16e78e00740ed73e79a3be76f))
* **core:** take no room for an empty list the host lowers ([f2fac23](https://github.com/elct9620/kobako/commit/f2fac236de645f69e479fa5bff97979005e344eb))
* **deps:** require a msgpack that sizes a container by its bytes ([a7d6a00](https://github.com/elct9620/kobako/commit/a7d6a008086412d4a30f14d41ab8c92bf6c1b02b))
* **ext:** fail a read whose Ruby Filesystem answers more than it asked ([5cfa780](https://github.com/elct9620/kobako/commit/5cfa780f47433370b5e9951a112153617d8a150d))
* **ext:** keep a host interrupt with the call it reached, on every exit ([886b6cf](https://github.com/elct9620/kobako/commit/886b6cf3f2eda2a589ee0cc40b6e19430a068706))
* **ext:** raise what a Ruby Filesystem's exclusive? raises from its mount ([6833e56](https://github.com/elct9620/kobako/commit/6833e56b2a9cb039c4893c0733a3976960754c50))
* **ext:** read an Errno class the one way for a Filesystem and a Mock ([b7d2a41](https://github.com/elct9620/kobako/commit/b7d2a4117ac2f7451589227fca92ba17c6ef89bb))
* **ext:** refuse a block called past its frame as a local jump, in one place ([04e4095](https://github.com/elct9620/kobako/commit/04e40953a9fe5fe8bcc5a94b2d6c4a0989a4b0c6))
* **ext:** refuse an endpoint object's port out of range as an argument error ([85e7c1d](https://github.com/elct9620/kobako/commit/85e7c1d9d98c0d9876111f823667964c0454be28))
* **ext:** take a duck-typed endpoint's defaults and an IPAddr address ([09a6830](https://github.com/elct9620/kobako/commit/09a6830c43beafc68287e2c9977053468d941126))
* **guest:** say a guest supports no block, as the glossary words an Unsupported ([3aa9a78](https://github.com/elct9620/kobako/commit/3aa9a7844780ba095fa2d197c3ab1a163a33dce6))
* **http:** answer a request whose name has no address with DNS-error ([295fe03](https://github.com/elct9620/kobako/commit/295fe036232e9256b2b3ce49f46168a51ce3f7e2))
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
* **io:** judge a lookup's family and flags in glibc's turn, and name IO's events ([fe19590](https://github.com/elct9620/kobako/commit/fe195909dfd348bf84e42464b0e9daad90872173))
* **io:** judge a UDP send's named peer by its spelling, and advice first ([17aa7c3](https://github.com/elct9620/kobako/commit/17aa7c3c1cac7f53cd04fa25750b75cbb22bce59))
* **io:** keep a socket's number and shut sides in the guest ([0e117f2](https://github.com/elct9620/kobako/commit/0e117f2956dfbc62535e1af8341d36078407e5db))
* **io:** leave File::Stat#&lt;=&gt; undefined, as a report it cannot order ([92dfbe0](https://github.com/elct9620/kobako/commit/92dfbe08d519ebc10cdaaadaac32478d287203e0))
* **io:** leave File::Stat#== undefined, as a report it cannot equate ([b517765](https://github.com/elct9620/kobako/commit/b517765f593dd84e5038e0a20eb8f16673ad83fa))
* **io:** let a UDP socket with no peer wait and keep its shut sides, as Linux's ([04f24de](https://github.com/elct9620/kobako/commit/04f24deed23192982bc271efccf36590bd2abc94))
* **io:** let the host judge a script's bind, as CRuby names it ([0ec0f80](https://github.com/elct9620/kobako/commit/0ec0f805b8a0c6caed0e4c7c25dd99c5cdaffd07))
* **io:** make UDPSocket an IO that IO.select waits on ([1cba17b](https://github.com/elct9620/kobako/commit/1cba17b7f2f7d7da626534bb9a1beed710741503))
* **io:** match a glob pattern by whole characters and its own spelling ([d1699c6](https://github.com/elct9620/kobako/commit/d1699c6d6f5584eb0873baebea358fe4f5cfd326))
* **io:** open a socket with a block as CRuby's IO.open does ([e6938fe](https://github.com/elct9620/kobako/commit/e6938fee87118570aa3027ac8f1257eb14b4d841))
* **io:** raise a Net::HTTP request body's failure as the Errno its code names ([3c3f7c7](https://github.com/elct9620/kobako/commit/3c3f7c7dce629efecb903b8a1b1bfc6f68adf053))
* **io:** raise EOPNOTSUPP for a socket failure WASI names no code for ([f019fdc](https://github.com/elct9620/kobako/commit/f019fdcc34d7e36f5dc57007f776201140475b9a))
* **io:** raise Errno::ENFILE for a host out of open files in system ([894f8b2](https://github.com/elct9620/kobako/commit/894f8b253d29db6bddad205be45a94327e35f2e5))
* **io:** raise IOError for a closed UDPSocket's connect and bind before any lookup ([b733892](https://github.com/elct9620/kobako/commit/b73389265149c5a3c87c63eb46e9d3c735b77651))
* **io:** raise IOError for a closed UDPSocket's send before any lookup ([114af24](https://github.com/elct9620/kobako/commit/114af24a67a968befb93314dc8535a3adf455d29))
* **io:** read a Float count or length the one way CRuby's NUM2LONG does ([794a529](https://github.com/elct9620/kobako/commit/794a529f3f9664f444d9c15e25964979daed429b))
* **io:** read a host as CRuby's host_str reads it ([ec86710](https://github.com/elct9620/kobako/commit/ec867108bdd872dfcb1d31e23fc93302157c52b7))
* **io:** read a lookup's family, kind, protocol, and flags in CRuby's order ([85422c4](https://github.com/elct9620/kobako/commit/85422c4b5f4232dd0e9058ff5183391f7473794b))
* **io:** read a Net::HTTP body into a destination once ([968278f](https://github.com/elct9620/kobako/commit/968278f8c1345e2bed9563835987eaf7482d584d))
* **io:** read a path from what answers to_path or to_str, as CRuby does ([2f9e094](https://github.com/elct9620/kobako/commit/2f9e094a74400fc4b373f22e46d9cf0f433f2c13))
* **io:** read a script's port as glibc keeps it in an int ([1d107b4](https://github.com/elct9620/kobako/commit/1d107b40510c8c5042840fea2e968f53e01fa340))
* **io:** read a script's socket and Net::HTTP arguments as CRuby's does ([8b4e3a3](https://github.com/elct9620/kobako/commit/8b4e3a30df09cf8b66930dbbcbf992e903b3777f))
* **io:** read a script's socket through what it holds, as CRuby's IO does ([0d38748](https://github.com/elct9620/kobako/commit/0d387489209580b0d7b33b003e72955141c8da66))
* **io:** read a socket stream's failure through network-error-code ([01761e1](https://github.com/elct9620/kobako/commit/01761e12d1e12484253611bb5ac2136ebff070f2))
* **io:** read a socket's characters one byte at a time, as CRuby's ([4cdc5cb](https://github.com/elct9620/kobako/commit/4cdc5cb412608798d9dd06bac0d05908ab0d454a))
* **io:** read a socket's port and keywords as CRuby on Linux does ([1df516e](https://github.com/elct9620/kobako/commit/1df516eb94e86a306c450fb5e08700eeae8a2272))
* **io:** read a timeout and exception: only where and as CRuby's does ([480720b](https://github.com/elct9620/kobako/commit/480720b97fc100dd93d7d54bf0333048fa8ab4f9))
* **io:** read a UDPSocket send's flags as CRuby's, refusing one WASI cannot carry ([ac229c4](https://github.com/elct9620/kobako/commit/ac229c48afa104562daefa068ce5a4278123725e))
* **io:** read an address family one way for every socket call ([a2d3c05](https://github.com/elct9620/kobako/commit/a2d3c058cf94348b2d5a8071b66815fdb10e443d))
* **io:** read an empty datagram, a nameless bind, and short options as Linux ([d7351f7](https://github.com/elct9620/kobako/commit/d7351f7e5ec8bcf057f56852769dac510b31496b))
* **io:** read an empty host and CRuby's bracketed names as literals in a lookup ([0a28696](https://github.com/elct9620/kobako/commit/0a286968ac9567148d35ce6bc04b521f61c8c9ba))
* **io:** read every C int argument as CRuby's NUM2INT bounds it ([0319f82](https://github.com/elct9620/kobako/commit/0319f828c0002b25728e23eb43ecafc20a4679c0))
* **io:** read every timeout as CRuby's rb_time_interval does, in one place ([4cd8158](https://github.com/elct9620/kobako/commit/4cd8158a590d272f89b973ca282cdb0a875c774d))
* **io:** read IO.select's timeout first, and connect_timeout only when a connect waits ([8fe66fa](https://github.com/elct9620/kobako/commit/8fe66fa29d2aeac61c11b964c7327f2737c8e434))
* **io:** read IO#wait, a lookup's order, and an Addrinfo array as CRuby ([983c268](https://github.com/elct9620/kobako/commit/983c268f09f57a325cb8bee04c910dfb1826be99))
* **io:** read socket and Net::HTTP arguments in CRuby's order ([34bc0c4](https://github.com/elct9620/kobako/commit/34bc0c4887c02410e3e4739ade38b823a7b3646e))
* **io:** read Socket.tcp's resolv_timeout: as the deadline CRuby's takes ([867492a](https://github.com/elct9620/kobako/commit/867492a9c8906a6b641326780da553e50229ec11))
* **io:** read what a script hands a lookup as CRuby and glibc read it ([ada8fbc](https://github.com/elct9620/kobako/commit/ada8fbcf8992ec2829a6e39d09716676bde5a1d2))
* **io:** receive from a TCPSocket's sender as CRuby's IPSocket does ([ad0b04a](https://github.com/elct9620/kobako/commit/ad0b04aae890a6b3abfce2d5c660e78ea1498b89))
* **io:** refuse a lookup's kind or protocol glibc has no entry for ([3cd1842](https://github.com/elct9620/kobako/commit/3cd18424abb2d63d966588682192440b55a02472))
* **io:** refuse a seek whence past C's int as CRuby's NUM2INT does ([972e3c8](https://github.com/elct9620/kobako/commit/972e3c8481eb6436b64da121dc21ad4555db933f))
* **io:** refuse an empty path instead of reading the working directory ([1970be2](https://github.com/elct9620/kobako/commit/1970be214fa8d956912fed505d9def61864b7d16))
* **io:** refuse NaN and infinite send flags, and read a UDP destination after its message ([724753f](https://github.com/elct9620/kobako/commit/724753f4faee8d054e00c4eda7f3bc2f559036f8))
* **io:** render a written value by its own to_s, as CRuby's rb_obj_as_string ([3c452f1](https://github.com/elct9620/kobako/commit/3c452f1d0c7f971acf799e1220da57d0dd6ba072))
* **io:** seek, putc, warn, and an unknown errno answer as CRuby's do ([86f8bff](https://github.com/elct9620/kobako/commit/86f8bffe826a01785edbdf6df142ae9353f30bac))
* **io:** send a script's Net::HTTP request as CRuby's net/http does ([787432d](https://github.com/elct9620/kobako/commit/787432d8d3eebbe7dc02ab12f46571189d267e3d))
* **io:** show an Integer service its port does not spell in Addrinfo#inspect ([b862f5c](https://github.com/elct9620/kobako/commit/b862f5cd981ed6faf3f963ae31d093eef5c80ffb))
* **io:** show and convert an Addrinfo as CRuby's does ([35017c2](https://github.com/elct9620/kobako/commit/35017c2695c5efde3991056188db3d8e52052c2c))
* **io:** shut a script's UDP socket through the socket itself ([bf9b70d](https://github.com/elct9620/kobako/commit/bf9b70d5bf91d4522a543cb28e12fcf60c266d32))
* **io:** take CRuby's TCPSocket arguments and bound Net::HTTP's waits ([892c27f](https://github.com/elct9620/kobako/commit/892c27f9b0bdbebd400535728b90b9da653b6f24))
* **io:** wait on a zero-length receive and name Addrinfo parts as CRuby's ([7bc3ffc](https://github.com/elct9620/kobako/commit/7bc3ffc052fd9b75271b7357b1367fc3994464b4))
* judge a request's authority, path, and headers as wasmtime does ([903cc7b](https://github.com/elct9620/kobako/commit/903cc7b39217f3038de6bab5981ea37499f25e1c))
* **mruby:** refuse a keyword name that is neither a Symbol nor a String ([2cf08b6](https://github.com/elct9620/kobako/commit/2cf08b6b7e12c0afdca2c833069f1c446459e438))
* **net:** abort a socket still waited on when its Invocation ends ([a1e8fd1](https://github.com/elct9620/kobako/commit/a1e8fd1bf2368e386812db0d637064c4675792b1))
* **net:** answer a dial with no local address as a DNS error ([954c486](https://github.com/elct9620/kobako/commit/954c4868bfe548c459b718897c22a5d3c7a8b0c7))
* **net:** answer a host down by its own name until a WASI code groups it ([1ec6001](https://github.com/elct9620/kobako/commit/1ec60013934486bf54fe2fa2e6fb6785009d4af1))
* **net:** answer a host socket's errno as wasmtime-wasi does on every host ([293ea2c](https://github.com/elct9620/kobako/commit/293ea2cccabc6d63840e199e796af419e2687658))
* **net:** answer a lookup's failure by its kind, as getaddrinfo does ([c70ae48](https://github.com/elct9620/kobako/commit/c70ae48796eb85f8ef62f9648acd1cd605070a02))
* **net:** answer a TLS handshake the server ends as connection-terminated ([7fa36bf](https://github.com/elct9620/kobako/commit/7fa36bf0b6121a27471134c7688550af5a479905))
* **net:** answer an IPv4-mapped address's lookup with its IPv4 address ([99f67c5](https://github.com/elct9620/kobako/commit/99f67c5da05bcc5d08222c8e505353475b68a93d))
* **net:** answer an unreachable request and a TLS alert as wasmtime-wasi-http does ([06bb9f9](https://github.com/elct9620/kobako/commit/06bb9f91c98bdb897853fc9fe0571ee20a6ee60d))
* **net:** answer connection-terminated for a connection cut mid-request ([bc7550c](https://github.com/elct9620/kobako/commit/bc7550c62e6fc12cbea1acc0f53349fc54a3e190))
* **net:** answer EAGAIN on a shut UDP read, as non-blocking Linux does ([675376d](https://github.com/elct9620/kobako/commit/675376d971c721ca23a5bd581e7d83db59dc94aa))
* **net:** answer preview2 permits, options, and schemes as wasmtime-wasi ([3489166](https://github.com/elct9620/kobako/commit/3489166945852ca53626b82510f5c0a728cf0b26))
* **net:** cancel a name lookup nobody waits for ([bf8c668](https://github.com/elct9620/kobako/commit/bf8c6682055f8993060de7c79c2d4af51a0bd111))
* **net:** cancel a Windows name lookup through GetAddrInfoExW ([db42e04](https://github.com/elct9620/kobako/commit/db42e042859bbafc32fd929cc9c8b9ddb4226513))
* **net:** close a guest's TCP socket the way Linux closes one ([c0f95e0](https://github.com/elct9620/kobako/commit/c0f95e0410edd7498bb4a0cea0039cacef1253a4))
* **net:** deny a socket without a network as wasmtime-wasi does ([09b0e9a](https://github.com/elct9620/kobako/commit/09b0e9a2713bd76ab0aafeaa5d459566891154e6))
* **net:** give a repeated real address one virtual address on every resolver ([c0d4666](https://github.com/elct9620/kobako/commit/c0d46667beff26588fba72e6b872ef0ed73c06af))
* **net:** hold a request's parts against memory while it is sent ([d1b642b](https://github.com/elct9620/kobako/commit/d1b642b1171d54b4c0d4cbc1ae6e8ff313b3dc89))
* **net:** hold a request's parts and a response's head against memory ([fbc93c5](https://github.com/elct9620/kobako/commit/fbc93c5d763396cb5b928801380ef3989ec931af))
* **net:** hold an HTTP request's place until its connection closes ([d6b9a14](https://github.com/elct9620/kobako/commit/d6b9a1404954f768caea6708eed045803651feec))
* **net:** judge a request's method in the case the guest sends it ([d74784a](https://github.com/elct9620/kobako/commit/d74784a464aa60493e3b1718eb9fd5dd47468c8a))
* **net:** judge an unaskable name before asking, and move past a server that fails ([646d022](https://github.com/elct9620/kobako/commit/646d0222596ac1bbb352e87d9a1227e2bb271669))
* **net:** judge and reach a connected peer the way the outbound path promises ([8a2797b](https://github.com/elct9620/kobako/commit/8a2797b867c886cf03cd123b1efc3011d7819bac))
* **net:** keep a released response's head against the Invocation's memory ([c808ec2](https://github.com/elct9620/kobako/commit/c808ec2c9f7d2507e8e1ae381fc3e2e00f8ea747))
* **net:** keep one virtual address per name, and judge a UDP send's peer by it ([8cc7afe](https://github.com/elct9620/kobako/commit/8cc7afe97956a25489bccfa82bf7d1ec7e750818))
* **net:** let a Protocol Family say whether a UDP socket repoints ([d96b0dc](https://github.com/elct9620/kobako/commit/d96b0dcfa32b7f636a360f471d7e0bcb7bc9d453))
* **net:** poll a TCP socket writable once its room is half what it queued ([a1873ee](https://github.com/elct9620/kobako/commit/a1873eead26a9a69f24bfe2d4d17ed9806d758cc))
* **net:** read a host the guest names as wasmtime-wasi's lookup does ([2defe86](https://github.com/elct9620/kobako/commit/2defe8666e9046179dab4ff8b74057e69c9ab10a))
* **net:** read DNS replies and resolver files as glibc's stub does ([ca6ed41](https://github.com/elct9620/kobako/commit/ca6ed41d4bc5384f1b0a78eda49045971c5748d4))
* **net:** refuse a request whose target is not in origin form ([4d02252](https://github.com/elct9620/kobako/commit/4d02252ec6a5c4a714f99cc67f4f79bbab95558a))
* **net:** resolve names on Linux with a stub resolver that can be cancelled ([cd3e4eb](https://github.com/elct9620/kobako/commit/cd3e4eb0b6041c47ce01c29c844299cf0e90c745))
* **net:** start a request's name lookup on the thread that sends it ([20c0d8c](https://github.com/elct9620/kobako/commit/20c0d8c1df7752e79df1fc98132be9dd4a6373d1))
* **net:** take a UDP peer's datagrams in before repointing, and refuse an empty label ([a67fa20](https://github.com/elct9620/kobako/commit/a67fa200521fe16379c0db5afb556268a0f606a7))
* **net:** take a UDP socket's datagrams in bounded turns, counting what each holds ([3362df2](https://github.com/elct9620/kobako/commit/3362df24070231ecaa8aec4f556309a02e5b3949))
* **net:** take a UDP socket's turn again once it spends its bound ([2711e0d](https://github.com/elct9620/kobako/commit/2711e0dd45290758bea28031c1cde5c5797ff593))
* **net:** wait on a socket by its own flag, and reset a released one sent to ([9dbac06](https://github.com/elct9620/kobako/commit/9dbac065500e78c84a2ae000f06064f6f21b17f1))
* **net:** wait on a socket whose non-blocking flag was cleared ([ad36e6c](https://github.com/elct9620/kobako/commit/ad36e6c41011f3cebe4f3b653a2bef8c05dbbf21))
* **network:** answer an endpoint the ruleset refuses as the Host App's argument ([1c6d710](https://github.com/elct9620/kobako/commit/1c6d710855495f874761eef1933f51c93a7a62b0))
* **network:** close a TCP socket once both ends sent their FIN, as tcp_done does ([db99d3b](https://github.com/elct9620/kobako/commit/db99d3b7128343fb307ef8f95e9fa97f03c8c275))
* **network:** compose a Ruby entry's block through the runtime's Builder ([f62a94a](https://github.com/elct9620/kobako/commit/f62a94a9cc1e95df05fb11e41f51679e48707b11))
* **network:** count each invocation's open connections apart ([dc426f6](https://github.com/elct9620/kobako/commit/dc426f60595b2df4ff508e7534ae37b7afed502c))
* **network:** hand a request policy each header name in lower case ([fd8efae](https://github.com/elct9620/kobako/commit/fd8efaeb74340dcc9ba224020ffd596b21a7f4af))
* **network:** judge a path rule in work bounded by the path ([20f37fc](https://github.com/elct9620/kobako/commit/20f37fcece42d3aea0018c987948a0814448b38e))
* **network:** read a credential's header name back as the Host App wrote it ([ea75433](https://github.com/elct9620/kobako/commit/ea75433bbf634cbcb6a658b3bef137b700027126))
* **network:** read a Mock's names with the URL host parser ([a170e19](https://github.com/elct9620/kobako/commit/a170e197a85a1f7ca1dd6d00d62cb7dfa33b39b7))
* **network:** read a socket's own end at a virtual port, not the host's ([1ef8486](https://github.com/elct9620/kobako/commit/1ef84864b6f8106e25eaa5fd0abf7310d56835c6))
* **network:** refuse a denied path in another case or with a trailing slash ([ff727bc](https://github.com/elct9620/kobako/commit/ff727bc5307f9739a54878d45e2359868877e4e8))
* **network:** refuse a Mock's dial with an errno that reads alike on every host ([3a7b705](https://github.com/elct9620/kobako/commit/3a7b705d82b195b4de1c54bc56fd04464dadba11))
* **network:** refuse a request carrying a credential's placeholder more than once ([03caf31](https://github.com/elct9620/kobako/commit/03caf319a318bbdec14273822dfcd612256a6b84))
* **network:** refuse a request whose credential cannot answer ([5b1f0c2](https://github.com/elct9620/kobako/commit/5b1f0c2abe8e2738aa3263ff3027245e7b654fb7))
* **network:** refuse a request whose method is not a token before any policy ([23a8e75](https://github.com/elct9620/kobako/commit/23a8e75d573b14610bdb048a0a3600017be0d69a))
* **network:** refuse an endpoint's text or a dependency of another class as TypeError ([e01c0f7](https://github.com/elct9620/kobako/commit/e01c0f7ca2474890321eadd8d2f4d31b6f6e89d3))
* **options:** refuse a deadline too far to count instead of aborting ([5611464](https://github.com/elct9620/kobako/commit/561146484100fa67da8d30ce6942b14e87237ab2))
* **parity:** run a scenario naming no cap under each cap's default ([3e306d9](https://github.com/elct9620/kobako/commit/3e306d987f7ca6d56aae1ab5f1ec812664e52ef5))
* **pool:** keep every slot through an interrupt, and each set up alike ([43660b1](https://github.com/elct9620/kobako/commit/43660b1de7b9161b50e84a12f6836a2036165854))
* refuse a network entry by the native rules alone, where it is written ([49568a6](https://github.com/elct9620/kobako/commit/49568a6cf0f1c45eb08e92c2dfccbfc907634e62))
* **regexp:** match each quantified unit beside an optional one for its own count ([4a5915e](https://github.com/elct9620/kobako/commit/4a5915e6331fdf29cbd61fb50607396a165de28d))
* **regexp:** read a backreference name past its first character, not its first byte ([51ce34f](https://github.com/elct9620/kobako/commit/51ce34fcc755b235ecfcb5b510c0817f3e1f3379))
* **release:** move kobako-codec's kobako-transport pin with each release ([91510e8](https://github.com/elct9620/kobako/commit/91510e8000ecc86afe5518b04b976ca2d77f1508))
* **ruby:** end a wait at once when an interrupt came while the guest computed ([52d919a](https://github.com/elct9620/kobako/commit/52d919a7fe23b587b8844884e06dd348ce6ecf4c))
* **runtime:** answer a host refusal by the errno it holds, as Linux does ([637e02b](https://github.com/elct9620/kobako/commit/637e02b9a2a037b7d85e8216128c02c702d6c50a))
* **runtime:** charge empty datagrams and virtual addresses ([9a4d9ba](https://github.com/elct9620/kobako/commit/9a4d9ba92cbd3ba5f7570f1693282202db43b6d9))
* **runtime:** charge the guest's growth and host-held memory to one budget ([f19659e](https://github.com/elct9620/kobako/commit/f19659ed93d0f5b0c1472dbc3bdbbb6815efbb4f))
* **runtime:** check a socket's state before its connect is translated or judged ([044f471](https://github.com/elct9620/kobako/commit/044f471f82aa5ac9edae39fae6dfae31be4e3dbf))
* **runtime:** finish an Overlay's cut-short workdir cleanup before its next change ([5f04225](https://github.com/elct9620/kobako/commit/5f04225a4f584bff876d7ad059f61a67b376da86))
* **runtime:** follow Linux's inet socket through a connect that fails ([596febd](https://github.com/elct9620/kobako/commit/596febdbce08a45f5fc49b713977af5acc664702))
* **runtime:** give a socket its virtual port as Linux's inet layer does ([2c80c02](https://github.com/elct9620/kobako/commit/2c80c027801b5a00501fb8141e8ebde53879a855))
* **runtime:** give a Tmpfs whiteout an inode and times of its own ([34d8c31](https://github.com/elct9620/kobako/commit/34d8c312ba9864dd463af78a31589c93f5589cd0))
* **runtime:** give back a TCP socket's unnamed port once a connect fails ([03a5d7f](https://github.com/elct9620/kobako/commit/03a5d7fda454ba42a6a6798c1698b282be1c0f4e))
* **runtime:** give each real address its own virtual address and dial one destination ([71c1b8d](https://github.com/elct9620/kobako/commit/71c1b8d7c3ee5ee34f57e5f1a48b43619176d129))
* **runtime:** judge a connect in Linux's order inside the runtime ([f5ede3a](https://github.com/elct9620/kobako/commit/f5ede3af5e8329c21f7630066c1ec539863ef433))
* **runtime:** judge the request target the server reads ([9bc185e](https://github.com/elct9620/kobako/commit/9bc185e6f00375498f4d9119a34357e32d52279b))
* **runtime:** keep a deadline in a forked child with a ticker of its own ([030a3fa](https://github.com/elct9620/kobako/commit/030a3fa9360b93c0fcee4ba77c2f110ddf8d4b13))
* **runtime:** keep a UDP socket's port as it connects again ([070cf8a](https://github.com/elct9620/kobako/commit/070cf8a50c6b8cc93816b5a34711c3910d13333e))
* **runtime:** keep an interrupt to the run of the call it reached ([fa61002](https://github.com/elct9620/kobako/commit/fa610027b2d19fd746a4197340a1aa768964bc42))
* **runtime:** leave a parent watching a socket its forked child lets go ([25161bf](https://github.com/elct9620/kobako/commit/25161bfb079a3cee169bafbe11a5fafe7471adf5))
* **runtime:** name a host failure the same way through a Bind as through a socket ([8d30507](https://github.com/elct9620/kobako/commit/8d30507987771fb94892c2388e19dcfb52f30d04))
* **runtime:** name a host out of quota, links, or a busy program as Linux does ([6f5f816](https://github.com/elct9620/kobako/commit/6f5f816a6192dec2ea21c219a3a2f2497a6b2aac))
* **runtime:** read a UDP socket's own queue alone, leaving the rest to the reactor's next turn ([b1a9208](https://github.com/elct9620/kobako/commit/b1a92088e56c2f9f934aa4e7f6963f497d30c456))
* **runtime:** refuse a cap of zero in one place both frontends ask ([aef531e](https://github.com/elct9620/kobako/commit/aef531eda4f2b32bc4748b0d09b98058331af3b6))
* **runtime:** refuse a connect when the host's addresses are unread ([47b1cd0](https://github.com/elct9620/kobako/commit/47b1cd0d168a6baa8c94bc5da85513bd3dc7e878))
* **runtime:** refuse a path servers normalize differently ([ba45e4f](https://github.com/elct9620/kobako/commit/ba45e4f5a48bf8f28e4e812465ad122bcbd8f72d))
* **runtime:** stop an Overlay copy up between chunks once its run must end ([855dfcc](https://github.com/elct9620/kobako/commit/855dfcc0a91d02730527b3219bc209c0231738a4))
* **runtime:** take a datagram past the receive buffer into an empty queue ([8f2f670](https://github.com/elct9620/kobako/commit/8f2f670619c7997e0c75a7869e1d5592b4c0e50a))
* **runtime:** tell a Bind's entries apart by identity on Windows too ([7f42b5c](https://github.com/elct9620/kobako/commit/7f42b5c264ab095866beed471a433ed28e5830ea))
* **sandbox:** name a Ractor's require in the warning a released main-Thread run gives ([71905d5](https://github.com/elct9620/kobako/commit/71905d519a32ca8bc9a2a8360e6560498b6ffca9))
* **sandbox:** raise a #run argument that cannot cross as CRuby's core classes ([2a3e721](https://github.com/elct9620/kobako/commit/2a3e721e926fb533e0cf0515ae82802bd285de40))
* **sandbox:** raise a Host App's mistake under CRuby's core classes ([daac61d](https://github.com/elct9620/kobako/commit/daac61d4831ecc0fd33d3218713a9ccb134beac9))
* **sandbox:** refuse a Service path or network name of another class as TypeError ([a2da0fe](https://github.com/elct9620/kobako/commit/a2da0fec8243ae6cae84bcd4945c96eb1e018c78))
* **sandbox:** refuse evaluating source whose bytes are not UTF-8 ([83406b7](https://github.com/elct9620/kobako/commit/83406b7dfb9e7b4d09fb68c248469ac9a303a008))
* **sdk:** refuse a run argument carrying a Handle the Host App wrote ([c094a9b](https://github.com/elct9620/kobako/commit/c094a9b137d8090b137f4cd12210ee23d28cfc73))
* **security:** fail the run when a nested mount's last name is missing ([757b654](https://github.com/elct9620/kobako/commit/757b654aa56336f72496f6b4f23de3635429849c))
* **security:** hold every name a nested mount's path leads through ([ae77f10](https://github.com/elct9620/kobako/commit/ae77f10589678601a1cd18807eff9d077938c86c))
* **security:** hold every name the walk placing a mount stepped onto ([ef9970e](https://github.com/elct9620/kobako/commit/ef9970ec6fa3b0b291a1fbd8873c4267b3465218))
* **security:** make no Cred for a Mount that stands nowhere ([c2c6190](https://github.com/elct9620/kobako/commit/c2c619095eb452d19be1aab65941b7cb86b339dd))
* **security:** refuse to move a directory above a nested mount ([e81193e](https://github.com/elct9620/kobako/commit/e81193e5259a296f71842053258cd78bf62487bf))
* **sig:** let Mock#route and #refuse name their protocol ([1982f06](https://github.com/elct9620/kobako/commit/1982f06a53dcce6e5f4941b2684fac5bca265aff))
* **transport:** call a bound BasicObject through Kernel's own methods ([46476bd](https://github.com/elct9620/kobako/commit/46476bd26d3b673b81fd5d24b771e111e90f25f5))
* **transport:** route a Ruby dispatch before reading its arguments ([10c7635](https://github.com/elct9620/kobako/commit/10c76356d106ccd8f2da615075de93170b21a086))
* **vfs:** ask a mount moved onto its name again as later mounts land ([515e944](https://github.com/elct9620/kobako/commit/515e944addca4b173cdd60b00949764e9f44e91e))
* **vfs:** fail the run when a nested mount stands nowhere ([b6e44fa](https://github.com/elct9620/kobako/commit/b6e44fa43723831b2c7d37c64bbfea95f2086b83))
* **vfs:** find a directory's inode in use by its identity ([d817440](https://github.com/elct9620/kobako/commit/d817440b61ca50a192f0f8f1b3d1fd03fe80595d))
* **vfs:** find a mount point busy only once a removal or move may go ahead ([468e539](https://github.com/elct9620/kobako/commit/468e539513238a3235140f23b2b9a4f7703e1118))
* **vfs:** keep a mount on its name where its link leads nowhere it can stand ([5100c3d](https://github.com/elct9620/kobako/commit/5100c3de9bfff5e9a2a17f981f8ea90bc9037b93))
* **vfs:** keep every mount reached as later mounts are placed ([1e550e6](https://github.com/elct9620/kobako/commit/1e550e684e857d524a72ea56a3669cf94c310e5e))
* **vfs:** place each mount as Linux's mount does, and open its preopen there ([a750bc7](https://github.com/elct9620/kobako/commit/a750bc75b3f1e8bcaf62ecaca1227fc0c92914c4))
* **vfs:** preopen only the outermost mounts and place the rest as Linux does ([7d21f34](https://github.com/elct9620/kobako/commit/7d21f348ab237497f4a509e464563f6c472a8a17))
* **vfs:** reach a nested mount however the guest spells its path ([73fb96a](https://github.com/elct9620/kobako/commit/73fb96ab63eaf01c73aaef1c803424513b2b07fd))
* **vfs:** show the later of two mounts standing on one directory ([baa4f16](https://github.com/elct9620/kobako/commit/baa4f16fba855d5383376c30cd442c6953b6ca58))
* **vfs:** stand a mount on the directory a link at its path leads to ([6f7d8ef](https://github.com/elct9620/kobako/commit/6f7d8eff35269165014300ea36b1af9db4d24f21))
* **vfs:** stand a mount on the name its path spells again ([5ed6e51](https://github.com/elct9620/kobako/commit/5ed6e51ebc6b146d13c39fe1d7722a9c054b5712))
* **vfs:** stand a mount only where a walk of its own path reaches it ([f9138b0](https://github.com/elct9620/kobako/commit/f9138b0eee7eea5298b4dced99428ed0492a7319))
* **vfs:** stand a nested mount on its mount point's dentry ([9ae2dd8](https://github.com/elct9620/kobako/commit/9ae2dd8b2403bd62faa62208d05a704d00de709a))
* **wasmtime:** answer a connect, a short write, and a datagram's sender as wasmtime-wasi ([d1762d0](https://github.com/elct9620/kobako/commit/d1762d0a6f703c003cdb8bf00c81628f61ce2b6f))
* **wasmtime:** answer a lookup or request past the file limit ([82d9f34](https://github.com/elct9620/kobako/commit/82d9f344a1a322d3b3bb811a8bbf39b4f98ad21f))
* **wasmtime:** answer closed to a blocking flush of nothing once the send side shut ([284d5a0](https://github.com/elct9620/kobako/commit/284d5a0ba7d30ba022d27ba60ed6cc2d4fb0a3c9))
* **wasmtime:** answer OVERFLOW for a file time a preview1 timestamp cannot hold ([a716dbf](https://github.com/elct9620/kobako/commit/a716dbf2152dc83945cdeaafc638c6f504fd3491))
* **wasmtime:** answer three preview2 socket and stream steps as wasmtime-wasi ([702addd](https://github.com/elct9620/kobako/commit/702adddb0a2626117303bcfcc9d80eb4f0bf938e))
* **wasmtime:** answer three socket calls as wasmtime-wasi does ([e0b8c8b](https://github.com/elct9620/kobako/commit/e0b8c8b45d1b2e7b7152b6332c49b5248bd0253d))
* **wasmtime:** ask a stream for room before a blocking flush waits ([1dbf02b](https://github.com/elct9620/kobako/commit/1dbf02bdc8309344366f40245cad5a3198bcf74b))
* **wasmtime:** ask a UDP address only what wasmtime asks, and its size first ([934d2eb](https://github.com/elct9620/kobako/commit/934d2eb8b7034db263423b8ad925a56ea731b1f4))
* **wasmtime:** bound what one preview2 call lifts by its hostcall fuel ([99ebca7](https://github.com/elct9620/kobako/commit/99ebca726a803d2de0b12bfcbcf7382bf7ab1308))
* **wasmtime:** charge each preview2 handle to the Invocation's memory ([def575a](https://github.com/elct9620/kobako/commit/def575a581b777e1a4c7bcb3604d3036b01b6284))
* **wasmtime:** close a TCP input stream once its receive side shuts ([76d944a](https://github.com/elct9620/kobako/commit/76d944a966e152f9e16e9fc0d61b8765b65a7418))
* **wasmtime:** close an output stream once a call on it fails ([df6b96f](https://github.com/elct9620/kobako/commit/df6b96fc65e3fba8ad4bbf92edebbf35639547d2))
* **wasmtime:** derive preview1's errno from its name, and map two more to preview2 ([aabd599](https://github.com/elct9620/kobako/commit/aabd5997070291dbe5055ae6b075f85f890f674b))
* **wasmtime:** hand out only the datagrams of the peer a stream was made with ([88123ea](https://github.com/elct9620/kobako/commit/88123ea0f6f4d128ae7af8f43a9e0bf063c8c309))
* **wasmtime:** hold preview1's files in the preview2 resource table ([d447054](https://github.com/elct9620/kobako/commit/d447054dee66c3b234975ca6331dce3ed9a76d8e))
* **wasmtime:** judge a TCP socket's pollable by the connect under way as it is polled ([b89bccd](https://github.com/elct9620/kobako/commit/b89bccdba789d69dc7b69ad07a7176b0d310b963))
* **wasmtime:** lend a standard stream's handle to its descriptor ([74059ac](https://github.com/elct9620/kobako/commit/74059ac5a2651d2ffe8decc0b8feb528f49dad40))
* **wasmtime:** make every pollable a child of what it watches ([48a0d2c](https://github.com/elct9620/kobako/commit/48a0d2c1cab25232baf29a9632e9b8a76413d236))
* **wasmtime:** number preview2 handles apart and reach no socket from preview1 ([adb0341](https://github.com/elct9620/kobako/commit/adb03410fd12804ccdaccd94748c6d1415417407))
* **wasmtime:** ready a closed output stream's pollable at once ([4ebd41a](https://github.com/elct9620/kobako/commit/4ebd41a90c5ee96f9e3ca90eea2fcb6246eb6ef5))
* **wasmtime:** refuse a negative read or write offset before the descriptor ([9b99512](https://github.com/elct9620/kobako/commit/9b995124380f0571b08c0cb68585c2acacf3d91a))
* **wasmtime:** refuse a short socket write whole, before any byte leaves ([5be1ec0](https://github.com/elct9620/kobako/commit/5be1ec027474376ecfcadce6d280891efe1a0afa))
* **wasmtime:** round a keepalive time's part of a second up, as wasmtime's reaches the host ([31c5184](https://github.com/elct9620/kobako/commit/31c51843e4e6758179b05725334203915aa42611))
* **wasmtime:** trap a preview2 handle the guest does not hold as it is lifted ([f866c42](https://github.com/elct9620/kobako/commit/f866c4279ab54b581c0e827266c7b9ce75e4c371))
* **wasmtime:** trap on a misaligned preview2 return area ([6192d9e](https://github.com/elct9620/kobako/commit/6192d9e7db3d3f42ab2520b9a3a25d168bd341b6))
* **wasmtime:** wait once per handle a poll lists more than once ([682fb21](https://github.com/elct9620/kobako/commit/682fb21b0623d015ebd5de4b1c78628eba99fef2))
* **wasmtime:** wait through preview2's poll from poll_oneoff ([760a280](https://github.com/elct9620/kobako/commit/760a28052853cbdb2dc9aa49cda2d1883744b86b))


### Reverts

* **guest:** keep the ABI's realloc and the glossary's preloads as designed ([58fb5ce](https://github.com/elct9620/kobako/commit/58fb5ce76796f32dc70a48603af8115c8372bfa8))


### Code Refactoring

* **errors:** name the missing Guest Binary's error after what is missing ([ac51a5e](https://github.com/elct9620/kobako/commit/ac51a5e2e15fce77296f54b79d0cb879df560a3a))
* **errors:** read a failure's guest class as class_name ([43c4055](https://github.com/elct9620/kobako/commit/43c4055ba0840fba4dc718bec86391541a6cd142))
* **guest:** handle a downcall's request and return its Reply ([d1fea6d](https://github.com/elct9620/kobako/commit/d1fea6d07f6c060562877e46cc0809a8ce856633))
* **guest:** name the guest seams a Guest author calls by Rust's and the ABI's words ([f36c91d](https://github.com/elct9620/kobako/commit/f36c91db8bda69b5812a7982b6260b3a7d66abd6))
* **guest:** read an exception's backtrace by mruby's own name ([589f97e](https://github.com/elct9620/kobako/commit/589f97ef4297664b8407307b5c02b6cb3ef9cdbc))
* **network:** name each Policies setter after the policy it sets ([299bfe1](https://github.com/elct9620/kobako/commit/299bfe114dae25fe82de8d10a27db6b7de14f018))
* **options:** judge the profile, the scheduling mode, and the floor once, in the runtime ([4a0eed1](https://github.com/elct9620/kobako/commit/4a0eed18e8ef2e0002e0e61783667084c6fa2e62))
* **options:** take every option's default from the runtime ([1982afe](https://github.com/elct9620/kobako/commit/1982afe89a987a269e1d8c42f69207e05f64ee40))
* **pool:** size a Pool with size:, as connection_pool does ([11d4629](https://github.com/elct9620/kobako/commit/11d4629c3a2be79a4ae2143765ea00cf85dfb75e))
* **runtime:** name the errno type Errno at the crate root ([f608e31](https://github.com/elct9620/kobako/commit/f608e31e81d1be9ddc6bb2fc8f6a26e5acafced4))
* **runtime:** name the missing Guest Binary's setup failure after what is missing ([732b8be](https://github.com/elct9620/kobako/commit/732b8bece8e796bd36d5e350b623c5ab8db28349))
* **sdk:** ask a Receiver whether it responds to the guest in the third person ([f897fc2](https://github.com/elct9620/kobako/commit/f897fc2a537bf221672ee24d64cd670fda9759dd))
* **sdk:** name how long an Extension backend's object lives Lifetime ([110ba8a](https://github.com/elct9620/kobako/commit/110ba8ae08aa42054ff42abd9265e28144220224))
* **sdk:** name RunPayload's constructors with from_ ([6e95532](https://github.com/elct9620/kobako/commit/6e955327f6e9d7c29641f89ac2b1edb58ed24993))
* **security:** prepare an invocation's Cred as Linux's prepare_creds does ([e4f8f86](https://github.com/elct9620/kobako/commit/e4f8f86f3b25fd468f54dcc332e5767ac4ac9c32))
* **transport:** load a Driver from its file, and read an origin as a lookup that may miss ([4ec8d79](https://github.com/elct9620/kobako/commit/4ec8d792ddf14bb21cfd36437b64b2389ee6df89))
* **transport:** name a Call's reply CallReply beside YieldReply and CompileReply ([f34063e](https://github.com/elct9620/kobako/commit/f34063ee94edb723800b4a0670cf912aa1a445e0))
* **transport:** name the message cap by what it bounds ([133e1c1](https://github.com/elct9620/kobako/commit/133e1c151506a1f41d5f2d17ac2678648c6ed262))
* **vfs:** build a change, a call, and a device number by the names Rust and Linux use ([f5710f8](https://github.com/elct9620/kobako/commit/f5710f82f0452d8aeb7dd8ee7cc2b822ae7c14fe))
* **vfs:** read an inode's or a file's type as file_type ([48150f7](https://github.com/elct9620/kobako/commit/48150f75263e9c195a8751ba8a9654c029162aa6))
* **vfs:** spell a rename, a listing, and an open file's flags as Linux and Rust do ([5bc0175](https://github.com/elct9620/kobako/commit/5bc01757679c2b68c77e22dd86e3dac9f001f4b5))
</details>

<details><summary>kobako-core: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.20.0...kobako-core-v0.21.0) (2026-11-18)


###   BREAKING CHANGES

* **transport:** kobako_transport::envelope::Reply is CallReply.
* **guest:** call kobako_core::abi::handle and return_reply in place of serve and answer.
* **guest:** call Kobako::handle_id_of, Kobako::backtrace_of,
* **transport:** a Guest Binary built against ABI 5 is refused at construction. Rebuild it against kobako-core, whose export_guest! emits the new names. kobako-transport drops ABI_VERSION as a u32, FRAME_LEN_SIZE, MAX_FRAME_LEN, pack_ptr_len, and unpack_ptr_len, and names the ABI through ABI_VERSION ("0.6.0"), GUEST_INTERFACE, HOST_INTERFACE, DOWNCALL, UPCALL, and PREAMBLE. kobako-core drops alloc; answer returns the return area's address.

### Features

* **transport:** carry the Guest ABI in the Canonical ABI's core shape ([34b9560](https://github.com/elct9620/kobako/commit/34b956001a05e15fe29febd7098b2ceb054e1bab))


### Bug Fixes

* **core:** take no room for an empty list the host lowers ([f2fac23](https://github.com/elct9620/kobako/commit/f2fac236de645f69e479fa5bff97979005e344eb))
* **guest:** say a guest supports no block, as the glossary words an Unsupported ([3aa9a78](https://github.com/elct9620/kobako/commit/3aa9a7844780ba095fa2d197c3ab1a163a33dce6))


### Reverts

* **guest:** keep the ABI's realloc and the glossary's preloads as designed ([58fb5ce](https://github.com/elct9620/kobako/commit/58fb5ce76796f32dc70a48603af8115c8372bfa8))


### Code Refactoring

* **guest:** handle a downcall's request and return its Reply ([d1fea6d](https://github.com/elct9620/kobako/commit/d1fea6d07f6c060562877e46cc0809a8ce856633))
* **guest:** name the guest seams a Guest author calls by Rust's and the ABI's words ([f36c91d](https://github.com/elct9620/kobako/commit/f36c91db8bda69b5812a7982b6260b3a7d66abd6))
* **transport:** name a Call's reply CallReply beside YieldReply and CompileReply ([f34063e](https://github.com/elct9620/kobako/commit/f34063ee94edb723800b4a0670cf912aa1a445e0))
</details>

<details><summary>kobako-mruby: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.20.0...kobako-mruby-v0.21.0) (2026-11-18)


###   BREAKING CHANGES

* **guest:** call kobako_core::abi::handle and return_reply in place of serve and answer.
* **guest:** call Kobako::exc_backtrace in place of
* **guest:** call Kobako::handle_id_of, Kobako::backtrace_of,

### Features

* **transport:** spell once the Service and entrypoint names a Panic carries ([fd09028](https://github.com/elct9620/kobako/commit/fd09028dee973397c66eb2029f7fd75bf6e7ee26))


### Bug Fixes

* **mruby:** refuse a keyword name that is neither a Symbol nor a String ([2cf08b6](https://github.com/elct9620/kobako/commit/2cf08b6b7e12c0afdca2c833069f1c446459e438))


### Code Refactoring

* **guest:** handle a downcall's request and return its Reply ([d1fea6d](https://github.com/elct9620/kobako/commit/d1fea6d07f6c060562877e46cc0809a8ce856633))
* **guest:** name the guest seams a Guest author calls by Rust's and the ABI's words ([f36c91d](https://github.com/elct9620/kobako/commit/f36c91db8bda69b5812a7982b6260b3a7d66abd6))
* **guest:** read an exception's backtrace by mruby's own name ([589f97e](https://github.com/elct9620/kobako/commit/589f97ef4297664b8407307b5c02b6cb3ef9cdbc))
</details>

<details><summary>kobako-io: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.20.0...kobako-io-v0.21.0) (2026-11-18)


### Features

* **io:** give a script Addrinfo and its lookups ([e7b9b3c](https://github.com/elct9620/kobako/commit/e7b9b3c2773d4917b11a3c38084317ad1929049b))
* **io:** give a script's TCPSocket setsockopt and getsockopt for SO_KEEPALIVE ([34948a5](https://github.com/elct9620/kobako/commit/34948a529ad8bf141ed5a6862851fc61ea2e53f8))
* **io:** give the guest UDPSocket over wasi:sockets/udp ([8abaea5](https://github.com/elct9620/kobako/commit/8abaea5b33ffcb24f314f898c4a511d4bbcc708f))
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
* **io:** name the socket constants CRuby does, and wait as IO#wait does ([fb758d7](https://github.com/elct9620/kobako/commit/fb758d7d3cf950f2cb69c6daa1b3a235e6352e6e))
* **io:** raise OpenSSL::SSL::SSLError for TLS a request could not open ([c113bbe](https://github.com/elct9620/kobako/commit/c113bbe02b8e1bdfd90c20b4a6dc7bf031ead6fb))
* **io:** read a file's lines by its name through File.readlines and File.foreach ([84db4d8](https://github.com/elct9620/kobako/commit/84db4d81064a1f8d03dd1cbca45ba4abf3566bb4))
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
* **io:** judge a lookup's family and flags in glibc's turn, and name IO's events ([fe19590](https://github.com/elct9620/kobako/commit/fe195909dfd348bf84e42464b0e9daad90872173))
* **io:** judge a UDP send's named peer by its spelling, and advice first ([17aa7c3](https://github.com/elct9620/kobako/commit/17aa7c3c1cac7f53cd04fa25750b75cbb22bce59))
* **io:** keep a socket's number and shut sides in the guest ([0e117f2](https://github.com/elct9620/kobako/commit/0e117f2956dfbc62535e1af8341d36078407e5db))
* **io:** leave File::Stat#&lt;=&gt; undefined, as a report it cannot order ([92dfbe0](https://github.com/elct9620/kobako/commit/92dfbe08d519ebc10cdaaadaac32478d287203e0))
* **io:** leave File::Stat#== undefined, as a report it cannot equate ([b517765](https://github.com/elct9620/kobako/commit/b517765f593dd84e5038e0a20eb8f16673ad83fa))
* **io:** let a UDP socket with no peer wait and keep its shut sides, as Linux's ([04f24de](https://github.com/elct9620/kobako/commit/04f24deed23192982bc271efccf36590bd2abc94))
* **io:** let the host judge a script's bind, as CRuby names it ([0ec0f80](https://github.com/elct9620/kobako/commit/0ec0f805b8a0c6caed0e4c7c25dd99c5cdaffd07))
* **io:** make UDPSocket an IO that IO.select waits on ([1cba17b](https://github.com/elct9620/kobako/commit/1cba17b7f2f7d7da626534bb9a1beed710741503))
* **io:** match a glob pattern by whole characters and its own spelling ([d1699c6](https://github.com/elct9620/kobako/commit/d1699c6d6f5584eb0873baebea358fe4f5cfd326))
* **io:** name every errno as CRuby does and build SystemCallError as it does ([487fcb6](https://github.com/elct9620/kobako/commit/487fcb67affc89b445296b6f482cec791f438ccd))
* **io:** name nil, true, and false by value when a String is expected ([b0706ff](https://github.com/elct9620/kobako/commit/b0706ffe61bb39bffe4ff668e6149bac6598685d))
* **io:** open a socket with a block as CRuby's IO.open does ([e6938fe](https://github.com/elct9620/kobako/commit/e6938fee87118570aa3027ac8f1257eb14b4d841))
* **io:** raise a Net::HTTP request body's failure as the Errno its code names ([3c3f7c7](https://github.com/elct9620/kobako/commit/3c3f7c7dce629efecb903b8a1b1bfc6f68adf053))
* **io:** raise EOPNOTSUPP for a socket failure WASI names no code for ([f019fdc](https://github.com/elct9620/kobako/commit/f019fdcc34d7e36f5dc57007f776201140475b9a))
* **io:** raise Errno::ENFILE for a host out of open files in system ([894f8b2](https://github.com/elct9620/kobako/commit/894f8b253d29db6bddad205be45a94327e35f2e5))
* **io:** raise IOError for a closed UDPSocket's connect and bind before any lookup ([b733892](https://github.com/elct9620/kobako/commit/b73389265149c5a3c87c63eb46e9d3c735b77651))
* **io:** raise IOError for a closed UDPSocket's send before any lookup ([114af24](https://github.com/elct9620/kobako/commit/114af24a67a968befb93314dc8535a3adf455d29))
* **io:** raise the Errno wasi-libc maps for a socket's in-flight call ([14412d6](https://github.com/elct9620/kobako/commit/14412d6abec939bf220f804ada74f17bf3a41eff))
* **io:** read a file's mode, names, lines, and seeks as CRuby does ([aeb0125](https://github.com/elct9620/kobako/commit/aeb01252708fecab8d5ed968866ca1b368670f0b))
* **io:** read a Float count or length the one way CRuby's NUM2LONG does ([794a529](https://github.com/elct9620/kobako/commit/794a529f3f9664f444d9c15e25964979daed429b))
* **io:** read a Float whence as the Integer it truncates to ([5386576](https://github.com/elct9620/kobako/commit/5386576297a76be3c14cc8ce87d1640cab75346e))
* **io:** read a host as CRuby's host_str reads it ([ec86710](https://github.com/elct9620/kobako/commit/ec867108bdd872dfcb1d31e23fc93302157c52b7))
* **io:** read a lookup's family, kind, protocol, and flags in CRuby's order ([85422c4](https://github.com/elct9620/kobako/commit/85422c4b5f4232dd0e9058ff5183391f7473794b))
* **io:** read a Net::HTTP body into a destination once ([968278f](https://github.com/elct9620/kobako/commit/968278f8c1345e2bed9563835987eaf7482d584d))
* **io:** read a path from what answers to_path or to_str, as CRuby does ([2f9e094](https://github.com/elct9620/kobako/commit/2f9e094a74400fc4b373f22e46d9cf0f433f2c13))
* **io:** read a script's port as glibc keeps it in an int ([1d107b4](https://github.com/elct9620/kobako/commit/1d107b40510c8c5042840fea2e968f53e01fa340))
* **io:** read a script's socket and Net::HTTP arguments as CRuby's does ([8b4e3a3](https://github.com/elct9620/kobako/commit/8b4e3a30df09cf8b66930dbbcbf992e903b3777f))
* **io:** read a script's socket through what it holds, as CRuby's IO does ([0d38748](https://github.com/elct9620/kobako/commit/0d387489209580b0d7b33b003e72955141c8da66))
* **io:** read a socket stream's failure through network-error-code ([01761e1](https://github.com/elct9620/kobako/commit/01761e12d1e12484253611bb5ac2136ebff070f2))
* **io:** read a socket's characters one byte at a time, as CRuby's ([4cdc5cb](https://github.com/elct9620/kobako/commit/4cdc5cb412608798d9dd06bac0d05908ab0d454a))
* **io:** read a socket's port and keywords as CRuby on Linux does ([1df516e](https://github.com/elct9620/kobako/commit/1df516eb94e86a306c450fb5e08700eeae8a2272))
* **io:** read a timeout and exception: only where and as CRuby's does ([480720b](https://github.com/elct9620/kobako/commit/480720b97fc100dd93d7d54bf0333048fa8ab4f9))
* **io:** read a UDPSocket send's flags as CRuby's, refusing one WASI cannot carry ([ac229c4](https://github.com/elct9620/kobako/commit/ac229c48afa104562daefa068ce5a4278123725e))
* **io:** read an address family one way for every socket call ([a2d3c05](https://github.com/elct9620/kobako/commit/a2d3c058cf94348b2d5a8071b66815fdb10e443d))
* **io:** read an empty datagram, a nameless bind, and short options as Linux ([d7351f7](https://github.com/elct9620/kobako/commit/d7351f7e5ec8bcf057f56852769dac510b31496b))
* **io:** read an empty host and CRuby's bracketed names as literals in a lookup ([0a28696](https://github.com/elct9620/kobako/commit/0a286968ac9567148d35ce6bc04b521f61c8c9ba))
* **io:** read every C int argument as CRuby's NUM2INT bounds it ([0319f82](https://github.com/elct9620/kobako/commit/0319f828c0002b25728e23eb43ecafc20a4679c0))
* **io:** read every timeout as CRuby's rb_time_interval does, in one place ([4cd8158](https://github.com/elct9620/kobako/commit/4cd8158a590d272f89b973ca282cdb0a875c774d))
* **io:** read IO.select's timeout first, and connect_timeout only when a connect waits ([8fe66fa](https://github.com/elct9620/kobako/commit/8fe66fa29d2aeac61c11b964c7327f2737c8e434))
* **io:** read IO#wait, a lookup's order, and an Addrinfo array as CRuby ([983c268](https://github.com/elct9620/kobako/commit/983c268f09f57a325cb8bee04c910dfb1826be99))
* **io:** read socket and Net::HTTP arguments in CRuby's order ([34bc0c4](https://github.com/elct9620/kobako/commit/34bc0c4887c02410e3e4739ade38b823a7b3646e))
* **io:** read Socket.tcp's resolv_timeout: as the deadline CRuby's takes ([867492a](https://github.com/elct9620/kobako/commit/867492a9c8906a6b641326780da553e50229ec11))
* **io:** read what a script hands a lookup as CRuby and glibc read it ([ada8fbc](https://github.com/elct9620/kobako/commit/ada8fbcf8992ec2829a6e39d09716676bde5a1d2))
* **io:** receive from a TCPSocket's sender as CRuby's IPSocket does ([ad0b04a](https://github.com/elct9620/kobako/commit/ad0b04aae890a6b3abfce2d5c660e78ea1498b89))
* **io:** refuse a lookup's kind or protocol glibc has no entry for ([3cd1842](https://github.com/elct9620/kobako/commit/3cd18424abb2d63d966588682192440b55a02472))
* **io:** refuse a seek whence past C's int as CRuby's NUM2INT does ([972e3c8](https://github.com/elct9620/kobako/commit/972e3c8481eb6436b64da121dc21ad4555db933f))
* **io:** refuse an empty path instead of reading the working directory ([1970be2](https://github.com/elct9620/kobako/commit/1970be214fa8d956912fed505d9def61864b7d16))
* **io:** refuse NaN and infinite send flags, and read a UDP destination after its message ([724753f](https://github.com/elct9620/kobako/commit/724753f4faee8d054e00c4eda7f3bc2f559036f8))
* **io:** render a written value by its own to_s, as CRuby's rb_obj_as_string ([3c452f1](https://github.com/elct9620/kobako/commit/3c452f1d0c7f971acf799e1220da57d0dd6ba072))
* **io:** seek, putc, warn, and an unknown errno answer as CRuby's do ([86f8bff](https://github.com/elct9620/kobako/commit/86f8bffe826a01785edbdf6df142ae9353f30bac))
* **io:** send a script's Net::HTTP request as CRuby's net/http does ([787432d](https://github.com/elct9620/kobako/commit/787432d8d3eebbe7dc02ab12f46571189d267e3d))
* **io:** show an Integer service its port does not spell in Addrinfo#inspect ([b862f5c](https://github.com/elct9620/kobako/commit/b862f5cd981ed6faf3f963ae31d093eef5c80ffb))
* **io:** show and convert an Addrinfo as CRuby's does ([35017c2](https://github.com/elct9620/kobako/commit/35017c2695c5efde3991056188db3d8e52052c2c))
* **io:** shut a script's UDP socket through the socket itself ([bf9b70d](https://github.com/elct9620/kobako/commit/bf9b70d5bf91d4522a543cb28e12fcf60c266d32))
* **io:** take CRuby's TCPSocket arguments and bound Net::HTTP's waits ([892c27f](https://github.com/elct9620/kobako/commit/892c27f9b0bdbebd400535728b90b9da653b6f24))
* **io:** try each address a name leads to, as CRuby's sockets do ([e808f0a](https://github.com/elct9620/kobako/commit/e808f0a3926881bef820289a86e0a72257c74e22))
* **io:** wait on a zero-length receive and name Addrinfo parts as CRuby's ([7bc3ffc](https://github.com/elct9620/kobako/commit/7bc3ffc052fd9b75271b7357b1367fc3994464b4))
* **net:** answer a host down by its own name until a WASI code groups it ([1ec6001](https://github.com/elct9620/kobako/commit/1ec60013934486bf54fe2fa2e6fb6785009d4af1))
* **net:** answer an unreachable request and a TLS alert as wasmtime-wasi-http does ([06bb9f9](https://github.com/elct9620/kobako/commit/06bb9f91c98bdb897853fc9fe0571ee20a6ee60d))
* **net:** answer EAGAIN on a shut UDP read, as non-blocking Linux does ([675376d](https://github.com/elct9620/kobako/commit/675376d971c721ca23a5bd581e7d83db59dc94aa))
* **net:** bound an HTTP request by the timeouts request-options sets ([b0e35d8](https://github.com/elct9620/kobako/commit/b0e35d80a275ecb33679ff1967e05f476f155ce7))
* **net:** deny a socket without a network as wasmtime-wasi does ([09b0e9a](https://github.com/elct9620/kobako/commit/09b0e9a2713bd76ab0aafeaa5d459566891154e6))
* **net:** judge and reach a connected peer the way the outbound path promises ([8a2797b](https://github.com/elct9620/kobako/commit/8a2797b867c886cf03cd123b1efc3011d7819bac))
* **net:** keep one virtual address per name, and judge a UDP send's peer by it ([8cc7afe](https://github.com/elct9620/kobako/commit/8cc7afe97956a25489bccfa82bf7d1ec7e750818))
* **net:** name a host socket's failure from its errno where std names none ([dddffa4](https://github.com/elct9620/kobako/commit/dddffa4bdc22575e3af4b27809662b04127aafc6))
* **network:** close a TCP socket once both ends sent their FIN, as tcp_done does ([db99d3b](https://github.com/elct9620/kobako/commit/db99d3b7128343fb307ef8f95e9fa97f03c8c275))
* **runtime:** give each real address its own virtual address and dial one destination ([71c1b8d](https://github.com/elct9620/kobako/commit/71c1b8d7c3ee5ee34f57e5f1a48b43619176d129))
* **runtime:** name a host out of quota, links, or a busy program as Linux does ([6f5f816](https://github.com/elct9620/kobako/commit/6f5f816a6192dec2ea21c219a3a2f2497a6b2aac))
</details>

<details><summary>kobako-regexp: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.20.0...kobako-regexp-v0.21.0) (2026-11-18)


### Bug Fixes

* **regexp:** read a backreference name past its first character, not its first byte ([51ce34f](https://github.com/elct9620/kobako/commit/51ce34fcc755b235ecfcb5b510c0817f3e1f3379))
</details>

<details><summary>kobako-json: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.20.0...kobako-json-v0.21.0) (2026-11-18)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions
</details>

<details><summary>kobako-baker: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.20.0...kobako-baker-v0.21.0) (2026-11-18)


###   BREAKING CHANGES

* **transport:** a Guest Binary built against ABI 5 is refused at construction. Rebuild it against kobako-core, whose export_guest! emits the new names. kobako-transport drops ABI_VERSION as a u32, FRAME_LEN_SIZE, MAX_FRAME_LEN, pack_ptr_len, and unpack_ptr_len, and names the ABI through ABI_VERSION ("0.6.0"), GUEST_INTERFACE, HOST_INTERFACE, DOWNCALL, UPCALL, and PREAMBLE. kobako-core drops alloc; answer returns the return area's address.

### Features

* **transport:** carry the Guest ABI in the Canonical ABI's core shape ([34b9560](https://github.com/elct9620/kobako/commit/34b956001a05e15fe29febd7098b2ceb054e1bab))
</details>

<details><summary>kobako-transport: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.20.0...kobako-transport-v0.21.0) (2026-11-18)


###   BREAKING CHANGES

* **transport:** kobako_transport::envelope::Reply is CallReply.
* **transport:** call Driver::from_file in place of Driver::new;
* **transport:** a Guest Binary built against ABI 5 is refused at construction. Rebuild it against kobako-core, whose export_guest! emits the new names. kobako-transport drops ABI_VERSION as a u32, FRAME_LEN_SIZE, MAX_FRAME_LEN, pack_ptr_len, and unpack_ptr_len, and names the ABI through ABI_VERSION ("0.6.0"), GUEST_INTERFACE, HOST_INTERFACE, DOWNCALL, UPCALL, and PREAMBLE. kobako-core drops alloc; answer returns the return area's address.
* **transport:** kobako_transport::abi::MAX_DISPATCH_PAYLOAD is renamed to MAX_MESSAGE_LEN; its value is unchanged.

### Features

* **transport:** carry the Guest ABI in the Canonical ABI's core shape ([34b9560](https://github.com/elct9620/kobako/commit/34b956001a05e15fe29febd7098b2ceb054e1bab))
* **transport:** expose the Capability Handle id bound the Envelope holds ([8a48012](https://github.com/elct9620/kobako/commit/8a48012cbac62c6a4a67c0c47664ee03a8089605))
* **transport:** spell once the form of a name a guest interns ([95c10bc](https://github.com/elct9620/kobako/commit/95c10bcf7180fc0e5ef432a9935ce3f856c75c9f))
* **transport:** spell once the names a Panic carries that a Frontend reads ([d1fb291](https://github.com/elct9620/kobako/commit/d1fb291082abcc376fc17d2f53b2e7d69dbf5da2))
* **transport:** spell once the Service and entrypoint names a Panic carries ([fd09028](https://github.com/elct9620/kobako/commit/fd09028dee973397c66eb2029f7fd75bf6e7ee26))


### Bug Fixes

* **guest:** say a guest supports no block, as the glossary words an Unsupported ([3aa9a78](https://github.com/elct9620/kobako/commit/3aa9a7844780ba095fa2d197c3ab1a163a33dce6))
* **transport:** refuse a Call target past the wire's Handle id range ([8a9b169](https://github.com/elct9620/kobako/commit/8a9b1694f96dc0769fff3251a76c3e1cb52a4866))


### Code Refactoring

* **transport:** load a Driver from its file, and read an origin as a lookup that may miss ([4ec8d79](https://github.com/elct9620/kobako/commit/4ec8d792ddf14bb21cfd36437b64b2389ee6df89))
* **transport:** name a Call's reply CallReply beside YieldReply and CompileReply ([f34063e](https://github.com/elct9620/kobako/commit/f34063ee94edb723800b4a0670cf912aa1a445e0))
* **transport:** name the message cap by what it bounds ([133e1c1](https://github.com/elct9620/kobako/commit/133e1c151506a1f41d5f2d17ac2678648c6ed262))
</details>

<details><summary>kobako-codec: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.20.0...kobako-codec-v0.21.0) (2026-11-18)


###   BREAKING CHANGES

* **guest:** call Kobako::handle_id_of, Kobako::backtrace_of,

### Bug Fixes

* **codec:** charge a map's NaN keys for the comparisons they cost a host ([62eead4](https://github.com/elct9620/kobako/commit/62eead43663705a16e78e00740ed73e79a3be76f))
* **codec:** refuse a String or binary too long for a MessagePack length ([d699d50](https://github.com/elct9620/kobako/commit/d699d50982f1c00398ee21e7bc647f8436a57267))


### Code Refactoring

* **guest:** name the guest seams a Guest author calls by Rust's and the ABI's words ([f36c91d](https://github.com/elct9620/kobako/commit/f36c91db8bda69b5812a7982b6260b3a7d66abd6))
</details>

<details><summary>kobako-runtime: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.20.0...kobako-runtime-v0.21.0) (2026-11-18)


###   BREAKING CHANGES

* **transport:** kobako_transport::envelope::Reply is CallReply.
* **runtime:** kobako_runtime::network::{Ruleset, AddressSet, address_set_of} and kobako_runtime::net::{Transport, Connector, new_platform_tls} are no longer public.
* **security:** call kobako_runtime::security::prepare_creds in place of prepare_creds_within.
* **vfs:** call Rdir::iterate_shared and File::is_readable, is_writable, is_append, is_nonblock, is_dsync, and is_sync in place of the bare names.
* **vfs:** call Iattr::for_size, Iattr::for_times, Kiocb::new, and get_anon_bdev in place of Iattr::size, Iattr::times, Kiocb::at, and
* **vfs:** call Inode::file_type and File::file_type in place of kind.
* **runtime:** match SetupError::GuestBinaryNotFound in place of
* **network:** open_connection_limit bounds what one invocation holds open at once, not what all of a Sandbox's invocations hold together.
* **network:** an Entry's Policies are built with with_request_policy, with_resolve_policy, and with_connect_policy in place of with_request, with_resolve, and with_connect.
* **transport:** a Guest Binary built against ABI 5 is refused at construction. Rebuild it against kobako-core, whose export_guest! emits the new names. kobako-transport drops ABI_VERSION as a u32, FRAME_LEN_SIZE, MAX_FRAME_LEN, pack_ptr_len, and unpack_ptr_len, and names the ABI through ABI_VERSION ("0.6.0"), GUEST_INTERFACE, HOST_INTERFACE, DOWNCALL, UPCALL, and PREAMBLE. kobako-core drops alloc; answer returns the return area's address.
* **sdk:** `kobako::Options` and `kobako_wasmtime::Config` read and set every cap through `limits`: write `options.limits.timeout = &` where `options.timeout = &` was written, and likewise for `setup_timeout`, `memory_limit`, `stdout_limit`, `stderr_limit`, `open_file_limit`, `open_connection_limit`, and `connection_limit`.
* **transport:** kobako_transport::abi::MAX_DISPATCH_PAYLOAD is renamed to MAX_MESSAGE_LEN; its value is unchanged.
* **runtime:** `memory_limit` now bounds the guest's `memory.grow` and what the host holds for the run together, where a Filesystem written in Ruby had its directory listings counted against a limit of their own. A run that grew close to `memory_limit` and listed a large directory may now raise `Errno::ENOMEM` in the guest or `Kobako::MemoryLimitError`, and `Usage#memory_peak` includes the listings. Raise `memory_limit` by what those listings hold to keep the earlier headroom.
* **runtime:** `kobako_runtime::FilesystemError`, `kobako_runtime::vfs::FilesystemError`, and `kobako_runtime::filesystem::FilesystemError` are now `kobako_runtime::Errno`, also reachable as `kobako::Errno`. Rename the type in a Filesystem or security module; its variants are unchanged.

### Features

* bound a guest's connections per Sandbox, per run, and per process ([7a4f869](https://github.com/elct9620/kobako/commit/7a4f8692cbc788a23c582b90112cd0275739d496))
* **filesystem:** let a Host App mark a Filesystem only the guest changes ([b7e0c81](https://github.com/elct9620/kobako/commit/b7e0c81b7338bd16243a9157de5c8194227b95c8))
* **network:** route and refuse a Mock's dials by protocol, UDP included ([9e8b44d](https://github.com/elct9620/kobako/commit/9e8b44d425db205fe0bb2faccca0782acf1e1b4e))
* **runtime:** answer the guest's network from a table a test writes ([b3aba65](https://github.com/elct9620/kobako/commit/b3aba650760b8cff3113341500fa414b17f4dd76))
* **runtime:** end a guest's HTTPS on the host, trusting the OS's own store ([5809be2](https://github.com/elct9620/kobako/commit/5809be20fb1127f29811e97062e68cfb11972b82))
* **runtime:** hold a socket's hop limit, and a UDP socket's buffer sizes ([8cf7385](https://github.com/elct9620/kobako/commit/8cf7385474c9ef569a4fee7d025cc5887fe8bb8a))
* **runtime:** hold a TCP socket's keepalive probes and buffer sizes ([a435964](https://github.com/elct9620/kobako/commit/a435964d9d61fcfec7435061b230367008a25fc9))
* **runtime:** hold a TCP socket's keepalive setting for its host socket ([4562f51](https://github.com/elct9620/kobako/commit/4562f510685a92ae02050674968679e15171b637))
* **runtime:** judge a guest's HTTP request by its endpoint's rules and the request policies ([69de0da](https://github.com/elct9620/kobako/commit/69de0dac95d0dfd2ce8f76c8dc39fdd1f92235ef))
* **runtime:** make a Filesystem from its kind without wrapping it first ([e7eb8a0](https://github.com/elct9620/kobako/commit/e7eb8a00a37794226edce2aa7f1bdcab2ef6790b))
* **runtime:** stream datagrams with one peer through a UDP socket file ([f72fd2e](https://github.com/elct9620/kobako/commit/f72fd2e80c8a85acdd66a07b394b723124a5c6ed))
* **runtime:** swap an entry's credential into a request bound to its own endpoints ([4b64ccb](https://github.com/elct9620/kobako/commit/4b64ccb61bcae18907bf424d2539052e9588875e))
* **transport:** carry the Guest ABI in the Canonical ABI's core shape ([34b9560](https://github.com/elct9620/kobako/commit/34b956001a05e15fe29febd7098b2ceb054e1bab))
* **wasmtime:** take the network's Protocol Family and Resolver from Config ([56cc53f](https://github.com/elct9620/kobako/commit/56cc53fa56cd987666f6b18b16e5264e314d8ef8))


### Bug Fixes

* answer an oversized datagram EMSGSIZE from the socket itself ([02317bc](https://github.com/elct9620/kobako/commit/02317bcc8e1ef86aa4b4a5e7a3b63b297cb6aeab))
* bound a guest's header list by size and charge it to its memory ([80c9ee2](https://github.com/elct9620/kobako/commit/80c9ee2523e0a3ad5de78e769a45136bcbcb6dc8))
* **filesystem:** answer a Bind's macOS stored name that is not UTF-8 as an illegal sequence ([6b68ede](https://github.com/elct9620/kobako/commit/6b68edee7f48797104fb9c14948ebd9a79548e63))
* **filesystem:** answer a Bind's stored name that is not UTF-8 as an illegal sequence ([ca755a6](https://github.com/elct9620/kobako/commit/ca755a65576698c17ade2da337956fb2ef187382))
* **filesystem:** open on Linux only the file a Bind judged ([8073129](https://github.com/elct9620/kobako/commit/8073129219ee49c6b6d4fe6a1199e316bf4bbfa5))
* **http:** answer a request whose name has no address with DNS-error ([295fe03](https://github.com/elct9620/kobako/commit/295fe036232e9256b2b3ce49f46168a51ce3f7e2))
* **http:** refuse an outgoing body finished with trailers ([c3c8f1b](https://github.com/elct9620/kobako/commit/c3c8f1b3b6277865f119c04065109a8cb4e1ab76))
* **http:** send the trailers an outgoing body finishes with ([7792bcc](https://github.com/elct9620/kobako/commit/7792bccbedff4758b2ec85bae344c45769997518))
* **io:** answer a script's socket addr at the guest's own virtual address ([6e8891a](https://github.com/elct9620/kobako/commit/6e8891a9b242849f1d1d9cea825ab47480b69b0e))
* judge a request's authority, path, and headers as wasmtime does ([903cc7b](https://github.com/elct9620/kobako/commit/903cc7b39217f3038de6bab5981ea37499f25e1c))
* make each guest socket and its file at creation, as socket() does ([fca4311](https://github.com/elct9620/kobako/commit/fca4311060be90df2ec79c1d21bb3a9a5b0b36ca))
* **net:** abort a socket still waited on when its Invocation ends ([a1e8fd1](https://github.com/elct9620/kobako/commit/a1e8fd1bf2368e386812db0d637064c4675792b1))
* **net:** answer a dial with no local address as a DNS error ([954c486](https://github.com/elct9620/kobako/commit/954c4868bfe548c459b718897c22a5d3c7a8b0c7))
* **net:** answer a host down by its own name until a WASI code groups it ([1ec6001](https://github.com/elct9620/kobako/commit/1ec60013934486bf54fe2fa2e6fb6785009d4af1))
* **net:** answer a host socket's errno as wasmtime-wasi does on every host ([293ea2c](https://github.com/elct9620/kobako/commit/293ea2cccabc6d63840e199e796af419e2687658))
* **net:** answer a lookup's failure by its kind, as getaddrinfo does ([c70ae48](https://github.com/elct9620/kobako/commit/c70ae48796eb85f8ef62f9648acd1cd605070a02))
* **net:** answer a TLS handshake the server ends as connection-terminated ([7fa36bf](https://github.com/elct9620/kobako/commit/7fa36bf0b6121a27471134c7688550af5a479905))
* **net:** answer an IPv4-mapped address's lookup with its IPv4 address ([99f67c5](https://github.com/elct9620/kobako/commit/99f67c5da05bcc5d08222c8e505353475b68a93d))
* **net:** answer an unreachable request and a TLS alert as wasmtime-wasi-http does ([06bb9f9](https://github.com/elct9620/kobako/commit/06bb9f91c98bdb897853fc9fe0571ee20a6ee60d))
* **net:** answer connection-terminated for a connection cut mid-request ([bc7550c](https://github.com/elct9620/kobako/commit/bc7550c62e6fc12cbea1acc0f53349fc54a3e190))
* **net:** answer EAGAIN on a shut UDP read, as non-blocking Linux does ([675376d](https://github.com/elct9620/kobako/commit/675376d971c721ca23a5bd581e7d83db59dc94aa))
* **net:** answer preview2 permits, options, and schemes as wasmtime-wasi ([3489166](https://github.com/elct9620/kobako/commit/3489166945852ca53626b82510f5c0a728cf0b26))
* **net:** answer what the name server answered and sort as RFC 6724 does ([54fe183](https://github.com/elct9620/kobako/commit/54fe1838aadbbd9099298362944bdede7311be7b))
* **net:** ask a lost question again before taking half an answer ([0d8725e](https://github.com/elct9620/kobako/commit/0d8725efc9f8654f90ea6e9fc58000ac82d3fb5c))
* **net:** bound an HTTP request by the timeouts request-options sets ([b0e35d8](https://github.com/elct9620/kobako/commit/b0e35d80a275ecb33679ff1967e05f476f155ce7))
* **net:** bound each wait a request leaves without a timeout at 600 seconds ([bd0fb84](https://github.com/elct9620/kobako/commit/bd0fb848904a57fff6dbd8eff413915dc2eecc8c))
* **net:** cancel a name lookup nobody waits for ([bf8c668](https://github.com/elct9620/kobako/commit/bf8c6682055f8993060de7c79c2d4af51a0bd111))
* **net:** cancel a Windows name lookup through GetAddrInfoExW ([db42e04](https://github.com/elct9620/kobako/commit/db42e042859bbafc32fd929cc9c8b9ddb4226513))
* **net:** close a guest's TCP socket the way Linux closes one ([c0f95e0](https://github.com/elct9620/kobako/commit/c0f95e0410edd7498bb4a0cea0039cacef1253a4))
* **net:** dial an HTTP request's upstream through a TCP socket of the Invocation's own ([c4cb150](https://github.com/elct9620/kobako/commit/c4cb150f7cfe65db6cfa2cfc584002623b7b12cc))
* **net:** disarm a lookup's timer when the lookup is cancelled ([743a806](https://github.com/elct9620/kobako/commit/743a8065dfc98ce19d65f3a894ef1aecdc537b91))
* **net:** end a lookup whose socket cannot open, as res_send does ([f1c0869](https://github.com/elct9620/kobako/commit/f1c086972882f7fc112f217bba1ba1f075e34b0d))
* **net:** give a repeated real address one virtual address on every resolver ([c0d4666](https://github.com/elct9620/kobako/commit/c0d46667beff26588fba72e6b872ef0ed73c06af))
* **net:** hold a request's parts against memory while it is sent ([d1b642b](https://github.com/elct9620/kobako/commit/d1b642b1171d54b4c0d4cbc1ae6e8ff313b3dc89))
* **net:** hold a request's parts and a response's head against memory ([fbc93c5](https://github.com/elct9620/kobako/commit/fbc93c5d763396cb5b928801380ef3989ec931af))
* **net:** hold an HTTP request's place until its connection closes ([d6b9a14](https://github.com/elct9620/kobako/commit/d6b9a1404954f768caea6708eed045803651feec))
* **net:** hold one reactor timer per waiting task ([051b355](https://github.com/elct9620/kobako/commit/051b3559b19624f7c7d4e2f531574db1f77c8c6f))
* **net:** hold the seam's read buffer against the Invocation's memory ([b589454](https://github.com/elct9620/kobako/commit/b5894545e572e34e17cce06d59bfa08ea1688555))
* **net:** judge a request's method in the case the guest sends it ([d74784a](https://github.com/elct9620/kobako/commit/d74784a464aa60493e3b1718eb9fd5dd47468c8a))
* **net:** judge an unaskable name before asking, and move past a server that fails ([646d022](https://github.com/elct9620/kobako/commit/646d0222596ac1bbb352e87d9a1227e2bb271669))
* **net:** judge and reach a connected peer the way the outbound path promises ([8a2797b](https://github.com/elct9620/kobako/commit/8a2797b867c886cf03cd123b1efc3011d7819bac))
* **net:** keep a released response's head against the Invocation's memory ([c808ec2](https://github.com/elct9620/kobako/commit/c808ec2c9f7d2507e8e1ae381fc3e2e00f8ea747))
* **net:** keep one virtual address per name, and judge a UDP send's peer by it ([8cc7afe](https://github.com/elct9620/kobako/commit/8cc7afe97956a25489bccfa82bf7d1ec7e750818))
* **net:** let a Protocol Family say whether a UDP socket repoints ([d96b0dc](https://github.com/elct9620/kobako/commit/d96b0dcfa32b7f636a360f471d7e0bcb7bc9d453))
* **net:** name a host socket's failure from its errno where std names none ([dddffa4](https://github.com/elct9620/kobako/commit/dddffa4bdc22575e3af4b27809662b04127aafc6))
* **net:** never cancel a Windows lookup through a handle it already ended ([6fc0bd4](https://github.com/elct9620/kobako/commit/6fc0bd4bfca32bc166d26b7ca6feb2d5c21a24c1))
* **net:** poll a TCP socket writable once its room is half what it queued ([a1873ee](https://github.com/elct9620/kobako/commit/a1873eead26a9a69f24bfe2d4d17ed9806d758cc))
* **net:** read a host the guest names as wasmtime-wasi's lookup does ([2defe86](https://github.com/elct9620/kobako/commit/2defe8666e9046179dab4ff8b74057e69c9ab10a))
* **net:** read back the very failure an upstream socket answered ([2c5a158](https://github.com/elct9620/kobako/commit/2c5a1588d73c1d858008c99fd7cda7f41bb9fac3))
* **net:** read DNS replies and resolver files as glibc's stub does ([ca6ed41](https://github.com/elct9620/kobako/commit/ca6ed41d4bc5384f1b0a78eda49045971c5748d4))
* **net:** read EDESTADDRREQ and ENOPROTOOPT from a host socket's errno ([84f310e](https://github.com/elct9620/kobako/commit/84f310ee0e4102d6899d8144bc4b202ebe8b65cc))
* **net:** refuse a request whose target is not in origin form ([4d02252](https://github.com/elct9620/kobako/commit/4d02252ec6a5c4a714f99cc67f4f79bbab95558a))
* **net:** repoint a UDP socket within the family its route sends in ([a1d1696](https://github.com/elct9620/kobako/commit/a1d1696bad555b4af162f997ae0e47a32a495e1c))
* **net:** repoll a socket once, after the round that follows its request ([df2a0e2](https://github.com/elct9620/kobako/commit/df2a0e201442e3ebef4743fcc88e67597e1ead89))
* **net:** resolve names on Linux with a stub resolver that can be cancelled ([cd3e4eb](https://github.com/elct9620/kobako/commit/cd3e4eb0b6041c47ce01c29c844299cf0e90c745))
* **net:** send a request's Host as the endpoint judged it ([30cb26d](https://github.com/elct9620/kobako/commit/30cb26d4c66e00e3fffeff7d4824658c1c10bb61))
* **net:** start a request's name lookup on the thread that sends it ([20c0d8c](https://github.com/elct9620/kobako/commit/20c0d8c1df7752e79df1fc98132be9dd4a6373d1))
* **net:** start a TCP socket's send buffer at Linux's default until it connects ([ea560c8](https://github.com/elct9620/kobako/commit/ea560c8ecdb0af143f94581ed10147bdc3e0359a))
* **net:** take a UDP peer's datagrams in before repointing, and refuse an empty label ([a67fa20](https://github.com/elct9620/kobako/commit/a67fa200521fe16379c0db5afb556268a0f606a7))
* **net:** take a UDP socket's datagrams in bounded turns, counting what each holds ([3362df2](https://github.com/elct9620/kobako/commit/3362df24070231ecaa8aec4f556309a02e5b3949))
* **net:** take a UDP socket's turn again once it spends its bound ([2711e0d](https://github.com/elct9620/kobako/commit/2711e0dd45290758bea28031c1cde5c5797ff593))
* **net:** try the next address when a request's first one is refused at once ([ac04578](https://github.com/elct9620/kobako/commit/ac04578bd77cb666433fda285587451fc66d5692))
* **net:** wait on a socket by its own flag, and reset a released one sent to ([9dbac06](https://github.com/elct9620/kobako/commit/9dbac065500e78c84a2ae000f06064f6f21b17f1))
* **net:** wait on a socket whose non-blocking flag was cleared ([ad36e6c](https://github.com/elct9620/kobako/commit/ad36e6c41011f3cebe4f3b653a2bef8c05dbbf21))
* **network:** answer an endpoint the ruleset refuses as the Host App's argument ([1c6d710](https://github.com/elct9620/kobako/commit/1c6d710855495f874761eef1933f51c93a7a62b0))
* **network:** bind a credential a Basic pair carries encoded ([7b40467](https://github.com/elct9620/kobako/commit/7b40467218d347ca588a55d573a4457231782a5a))
* **network:** close a TCP socket once both ends sent their FIN, as tcp_done does ([db99d3b](https://github.com/elct9620/kobako/commit/db99d3b7128343fb307ef8f95e9fa97f03c8c275))
* **network:** compile a DNS endpoint like any other ([f47524b](https://github.com/elct9620/kobako/commit/f47524b888444314cd8762e45e4ef5ba4db92b64))
* **network:** count each invocation's open connections apart ([dc426f6](https://github.com/elct9620/kobako/commit/dc426f60595b2df4ff508e7534ae37b7afed502c))
* **network:** hand a request policy each header name in lower case ([fd8efae](https://github.com/elct9620/kobako/commit/fd8efaeb74340dcc9ba224020ffd596b21a7f4af))
* **network:** judge a path rule in work bounded by the path ([20f37fc](https://github.com/elct9620/kobako/commit/20f37fcece42d3aea0018c987948a0814448b38e))
* **network:** leave a refused hop limit out of a UDP connect's outcome ([0879854](https://github.com/elct9620/kobako/commit/087985403063cb1be453854f13d20e0822ba8fc8))
* **network:** never let an HTTP exchange go from inside its own task ([84b0ad9](https://github.com/elct9620/kobako/commit/84b0ad9ee5fca6be009e35cf7178503005d1b7f0))
* **network:** read a Mock's names with the URL host parser ([a170e19](https://github.com/elct9620/kobako/commit/a170e197a85a1f7ca1dd6d00d62cb7dfa33b39b7))
* **network:** read a socket's own end at a virtual port, not the host's ([1ef8486](https://github.com/elct9620/kobako/commit/1ef84864b6f8106e25eaa5fd0abf7310d56835c6))
* **network:** refuse a denied path in another case or with a trailing slash ([ff727bc](https://github.com/elct9620/kobako/commit/ff727bc5307f9739a54878d45e2359868877e4e8))
* **network:** refuse a Mock's dial with an errno that reads alike on every host ([3a7b705](https://github.com/elct9620/kobako/commit/3a7b705d82b195b4de1c54bc56fd04464dadba11))
* **network:** refuse a request carrying a credential's placeholder more than once ([03caf31](https://github.com/elct9620/kobako/commit/03caf319a318bbdec14273822dfcd612256a6b84))
* **network:** refuse a request whose credential cannot answer ([5b1f0c2](https://github.com/elct9620/kobako/commit/5b1f0c2abe8e2738aa3263ff3027245e7b654fb7))
* **network:** refuse a request whose method is not a token before any policy ([23a8e75](https://github.com/elct9620/kobako/commit/23a8e75d573b14610bdb048a0a3600017be0d69a))
* **network:** refuse an endpoint whose port was set to zero after it was built ([cac4605](https://github.com/elct9620/kobako/commit/cac46052eeffe272a7e5453e27aa8d97ff7e8409))
* **runtime:** answer a host refusal by the errno it holds, as Linux does ([637e02b](https://github.com/elct9620/kobako/commit/637e02b9a2a037b7d85e8216128c02c702d6c50a))
* **runtime:** answer a TCP socket's error, close, and shutdown as Linux does ([dd4c684](https://github.com/elct9620/kobako/commit/dd4c68478c431833d79073eda8de83fde020ba81))
* **runtime:** answer a UDP port unreachable as refused on Windows too ([32a94c3](https://github.com/elct9620/kobako/commit/32a94c3991265a6e967253d7d624c27282acc9c4))
* **runtime:** charge empty datagrams and virtual addresses ([9a4d9ba](https://github.com/elct9620/kobako/commit/9a4d9ba92cbd3ba5f7570f1693282202db43b6d9))
* **runtime:** charge the guest's growth and host-held memory to one budget ([f19659e](https://github.com/elct9620/kobako/commit/f19659ed93d0f5b0c1472dbc3bdbbb6815efbb4f))
* **runtime:** check a socket's state before its connect is translated or judged ([044f471](https://github.com/elct9620/kobako/commit/044f471f82aa5ac9edae39fae6dfae31be4e3dbf))
* **runtime:** class IPv6 addresses by IANA's special-purpose registry ([205ae65](https://github.com/elct9620/kobako/commit/205ae65e1924d65087e33472ba0074c840109932))
* **runtime:** connect a TCP socket again as Linux does ([7679d71](https://github.com/elct9620/kobako/commit/7679d71bd2bd71268b7da10a210be9522b032347))
* **runtime:** finish an Overlay's cut-short workdir cleanup before its next change ([5f04225](https://github.com/elct9620/kobako/commit/5f04225a4f584bff876d7ad059f61a67b376da86))
* **runtime:** follow Linux's inet socket through a connect that fails ([596febd](https://github.com/elct9620/kobako/commit/596febdbce08a45f5fc49b713977af5acc664702))
* **runtime:** give a socket its virtual port as Linux's inet layer does ([2c80c02](https://github.com/elct9620/kobako/commit/2c80c027801b5a00501fb8141e8ebde53879a855))
* **runtime:** give a Tmpfs whiteout an inode and times of its own ([34d8c31](https://github.com/elct9620/kobako/commit/34d8c312ba9864dd463af78a31589c93f5589cd0))
* **runtime:** give back a TCP socket's unnamed port once a connect fails ([03a5d7f](https://github.com/elct9620/kobako/commit/03a5d7fda454ba42a6a6798c1698b282be1c0f4e))
* **runtime:** give each Invocation's outbound seam its own TLS sessions ([75119d8](https://github.com/elct9620/kobako/commit/75119d88a49f546a2d827a9476c5067d99bc02dd))
* **runtime:** give each real address its own virtual address and dial one destination ([71c1b8d](https://github.com/elct9620/kobako/commit/71c1b8d7c3ee5ee34f57e5f1a48b43619176d129))
* **runtime:** hold a socket's buffers against the Invocation's memory ([5d629a7](https://github.com/elct9620/kobako/commit/5d629a71f1940222dac36a57502030025575ef70))
* **runtime:** hold a TCP socket's virtual port before it dials ([dff5cfe](https://github.com/elct9620/kobako/commit/dff5cfe788339146fda4ef3cb9b88c2cb6bd0302))
* **runtime:** judge a connect in Linux's order inside the runtime ([f5ede3a](https://github.com/elct9620/kobako/commit/f5ede3af5e8329c21f7630066c1ec539863ef433))
* **runtime:** judge IDN endpoints, Basic credentials, and request order as promised ([a1d4a35](https://github.com/elct9620/kobako/commit/a1d4a353f439cfa2fc5a9b11119f0bda622122ef))
* **runtime:** judge the guest's HTTP headers, binds, and peerless streams in the runtime ([bc3e024](https://github.com/elct9620/kobako/commit/bc3e024483e43d8b121cb67e661e06e99997f604))
* **runtime:** judge the request target the server reads ([9bc185e](https://github.com/elct9620/kobako/commit/9bc185e6f00375498f4d9119a34357e32d52279b))
* **runtime:** keep a deadline in a forked child with a ticker of its own ([030a3fa](https://github.com/elct9620/kobako/commit/030a3fa9360b93c0fcee4ba77c2f110ddf8d4b13))
* **runtime:** keep a failed growth in the memory peak ([2e2b986](https://github.com/elct9620/kobako/commit/2e2b986dfa47aff61f0c20ad9b63178bfccbd441))
* **runtime:** keep a name a Mock holds twice held until both holds drop ([26b362c](https://github.com/elct9620/kobako/commit/26b362cc9e61509f025ac6c5700bf3e95ed2b137))
* **runtime:** keep a refused connect's error when its options cannot be set ([258b51e](https://github.com/elct9620/kobako/commit/258b51e1d8398619da5cd6a1dcbd8e0ba42cf7d6))
* **runtime:** keep a socket's names for a host down to sockets ([6895bf9](https://github.com/elct9620/kobako/commit/6895bf97e7e1e2fa95d00e6602b39c16f3cb2f60))
* **runtime:** keep a TCP socket's options when its host socket refuses one ([53284a4](https://github.com/elct9620/kobako/commit/53284a4ad02da8dd23dd9402b070462f58b765aa))
* **runtime:** keep a UDP socket's port as it connects again ([070cf8a](https://github.com/elct9620/kobako/commit/070cf8a50c6b8cc93816b5a34711c3910d13333e))
* **runtime:** keep an interrupt to the run of the call it reached ([fa61002](https://github.com/elct9620/kobako/commit/fa610027b2d19fd746a4197340a1aa768964bc42))
* **runtime:** keep Nagle's algorithm on a guest's own TCP socket ([7db0695](https://github.com/elct9620/kobako/commit/7db06959f615300783b606a3cb7416c8b909980c))
* **runtime:** leave a parent watching a socket its forked child lets go ([25161bf](https://github.com/elct9620/kobako/commit/25161bfb079a3cee169bafbe11a5fafe7471adf5))
* **runtime:** let an HTTP exchange go once nothing holds it ([2d78df9](https://github.com/elct9620/kobako/commit/2d78df98d11cdd8671629b1da2a5ac4b0b1862e7))
* **runtime:** name a host failure the same way through a Bind as through a socket ([8d30507](https://github.com/elct9620/kobako/commit/8d30507987771fb94892c2388e19dcfb52f30d04))
* **runtime:** name a host failure through one table for files and sockets ([1490a1f](https://github.com/elct9620/kobako/commit/1490a1f7c5fdfa5a383a12bfb0e0b8a4fef3fcdd))
* **runtime:** name a host out of quota, links, or a busy program as Linux does ([6f5f816](https://github.com/elct9620/kobako/commit/6f5f816a6192dec2ea21c219a3a2f2497a6b2aac))
* **runtime:** name a local address another socket holds as EADDRINUSE ([6b39011](https://github.com/elct9620/kobako/commit/6b3901117e18568bd1c47d3f1a913df67f09ea35))
* **runtime:** read a UDP socket's own queue alone, leaving the rest to the reactor's next turn ([b1a9208](https://github.com/elct9620/kobako/commit/b1a92088e56c2f9f934aa4e7f6963f497d30c456))
* **runtime:** refuse a cap of zero in one place both frontends ask ([aef531e](https://github.com/elct9620/kobako/commit/aef531eda4f2b32bc4748b0d09b98058331af3b6))
* **runtime:** refuse a connect to an unmapped virtual address ([6a65311](https://github.com/elct9620/kobako/commit/6a653119a14d428df421157accf76ecb2ec04efb))
* **runtime:** refuse a connect when the host's addresses are unread ([47b1cd0](https://github.com/elct9620/kobako/commit/47b1cd0d168a6baa8c94bc5da85513bd3dc7e878))
* **runtime:** refuse a path servers normalize differently ([ba45e4f](https://github.com/elct9620/kobako/commit/ba45e4f5a48bf8f28e4e812465ad122bcbd8f72d))
* **runtime:** refuse a return area before the guest gives room for the answer ([8fe210c](https://github.com/elct9620/kobako/commit/8fe210ce77e4168a51842b57b05a076a65320a9f))
* **runtime:** refuse a second connect on a connecting or connected socket ([2949d40](https://github.com/elct9620/kobako/commit/2949d402303e11af2b4346a866fee754e1e00dbd))
* **runtime:** refuse an https request with no trust roots before dialing ([ba0995c](https://github.com/elct9620/kobako/commit/ba0995cd6dc3f5f9f6b0e74e8b0801348f35ebb5))
* **runtime:** report socket failures as Linux does and keep the reactor and resolver honest ([b7e9a0d](https://github.com/elct9620/kobako/commit/b7e9a0dfc55f32bbe2a844ff89c0bc4162429438))
* **runtime:** resolve a request's authority as a lookup holding its place ([fd62ff6](https://github.com/elct9620/kobako/commit/fd62ff6ba9a53ab66eb8a68037ae85bb95f01387))
* **runtime:** send a UDP datagram at once and answer its failures as Linux does ([6abbab0](https://github.com/elct9620/kobako/commit/6abbab0e0da88f4b05797396aa7c96b8e061b3ed))
* **runtime:** stop an Overlay copy up between chunks once its run must end ([855dfcc](https://github.com/elct9620/kobako/commit/855dfcc0a91d02730527b3219bc209c0231738a4))
* **runtime:** take a datagram past the receive buffer into an empty queue ([8f2f670](https://github.com/elct9620/kobako/commit/8f2f670619c7997e0c75a7869e1d5592b4c0e50a))
* **runtime:** take the reactor before a TCP socket dials ([e056522](https://github.com/elct9620/kobako/commit/e0565227c357d05bca12610cd26d4aaac85b604a))
* **runtime:** tell a Bind's entries apart by identity on Windows too ([7f42b5c](https://github.com/elct9620/kobako/commit/7f42b5c264ab095866beed471a433ed28e5830ea))
* **security:** fail the run when a nested mount's last name is missing ([757b654](https://github.com/elct9620/kobako/commit/757b654aa56336f72496f6b4f23de3635429849c))
* **security:** hold every name a nested mount's path leads through ([ae77f10](https://github.com/elct9620/kobako/commit/ae77f10589678601a1cd18807eff9d077938c86c))
* **security:** hold every name the walk placing a mount stepped onto ([ef9970e](https://github.com/elct9620/kobako/commit/ef9970ec6fa3b0b291a1fbd8873c4267b3465218))
* **security:** make no Cred for a Mount that stands nowhere ([c2c6190](https://github.com/elct9620/kobako/commit/c2c619095eb452d19be1aab65941b7cb86b339dd))
* **security:** refuse to move a directory above a nested mount ([e81193e](https://github.com/elct9620/kobako/commit/e81193e5259a296f71842053258cd78bf62487bf))
* **vfs:** ask a mount moved onto its name again as later mounts land ([515e944](https://github.com/elct9620/kobako/commit/515e944addca4b173cdd60b00949764e9f44e91e))
* **vfs:** ask a walk again when a move lands before a `..` out of a mount ([06b5bd8](https://github.com/elct9620/kobako/commit/06b5bd8e839dccde5dc25f4bd67df5f7058898ab))
* **vfs:** fail the run when a nested mount stands nowhere ([b6e44fa](https://github.com/elct9620/kobako/commit/b6e44fa43723831b2c7d37c64bbfea95f2086b83))
* **vfs:** find a directory's inode in use by its identity ([d817440](https://github.com/elct9620/kobako/commit/d817440b61ca50a192f0f8f1b3d1fd03fe80595d))
* **vfs:** find a mount point busy only once a removal or move may go ahead ([468e539](https://github.com/elct9620/kobako/commit/468e539513238a3235140f23b2b9a4f7703e1118))
* **vfs:** find a rename's source busy only when nothing stands at its name ([8a6416a](https://github.com/elct9620/kobako/commit/8a6416a69a48f7e6779be1ab53eff07d9d00ac0e))
* **vfs:** keep a dentry a move carried while its revalidation was asked ([cb20bb6](https://github.com/elct9620/kobako/commit/cb20bb6658d59e000d07fe6e9e299a0a0c5d3f35))
* **vfs:** keep a mount on its name where its link leads nowhere it can stand ([5100c3d](https://github.com/elct9620/kobako/commit/5100c3de9bfff5e9a2a17f981f8ea90bc9037b93))
* **vfs:** keep every mount reached as later mounts are placed ([1e550e6](https://github.com/elct9620/kobako/commit/1e550e684e857d524a72ea56a3669cf94c310e5e))
* **vfs:** place each mount as Linux's mount does, and open its preopen there ([a750bc7](https://github.com/elct9620/kobako/commit/a750bc75b3f1e8bcaf62ecaca1227fc0c92914c4))
* **vfs:** preopen only the outermost mounts and place the rest as Linux does ([7d21f34](https://github.com/elct9620/kobako/commit/7d21f348ab237497f4a509e464563f6c472a8a17))
* **vfs:** reach a nested mount however the guest spells its path ([73fb96a](https://github.com/elct9620/kobako/commit/73fb96ab63eaf01c73aaef1c803424513b2b07fd))
* **vfs:** show the later of two mounts standing on one directory ([baa4f16](https://github.com/elct9620/kobako/commit/baa4f16fba855d5383376c30cd442c6953b6ca58))
* **vfs:** stand a mount on the directory a link at its path leads to ([6f7d8ef](https://github.com/elct9620/kobako/commit/6f7d8eff35269165014300ea36b1af9db4d24f21))
* **vfs:** stand a mount on the name its path spells again ([5ed6e51](https://github.com/elct9620/kobako/commit/5ed6e51ebc6b146d13c39fe1d7722a9c054b5712))
* **vfs:** stand a mount only where a walk of its own path reaches it ([f9138b0](https://github.com/elct9620/kobako/commit/f9138b0eee7eea5298b4dced99428ed0492a7319))
* **vfs:** stand a nested mount on its mount point's dentry ([9ae2dd8](https://github.com/elct9620/kobako/commit/9ae2dd8b2403bd62faa62208d05a704d00de709a))
* **wasmtime:** answer a connect, a short write, and a datagram's sender as wasmtime-wasi ([d1762d0](https://github.com/elct9620/kobako/commit/d1762d0a6f703c003cdb8bf00c81628f61ce2b6f))
* **wasmtime:** give back the charge of a growth wasmtime then fails ([070c463](https://github.com/elct9620/kobako/commit/070c463e11428eb27465d6d9a9e7ba38b738249f))
* **wasmtime:** judge a TCP socket's pollable by the connect under way as it is polled ([b89bccd](https://github.com/elct9620/kobako/commit/b89bccdba789d69dc7b69ad07a7176b0d310b963))
* **wasmtime:** refuse a short socket write whole, before any byte leaves ([5be1ec0](https://github.com/elct9620/kobako/commit/5be1ec027474376ecfcadce6d280891efe1a0afa))


### Performance Improvements

* **vfs:** keep a walk's trail only where a mount is placed ([ab34a1a](https://github.com/elct9620/kobako/commit/ab34a1a373fceb54167545aeb0a9aea7ebd98af2))


### Code Refactoring

* **network:** name each Policies setter after the policy it sets ([299bfe1](https://github.com/elct9620/kobako/commit/299bfe114dae25fe82de8d10a27db6b7de14f018))
* **runtime:** keep the ruleset and the L7 transport inside the runtime ([18db50b](https://github.com/elct9620/kobako/commit/18db50b68c11fa0d8e7764fc40523453b1032852))
* **runtime:** name the errno type Errno at the crate root ([f608e31](https://github.com/elct9620/kobako/commit/f608e31e81d1be9ddc6bb2fc8f6a26e5acafced4))
* **runtime:** name the missing Guest Binary's setup failure after what is missing ([732b8be](https://github.com/elct9620/kobako/commit/732b8bece8e796bd36d5e350b623c5ab8db28349))
* **sdk:** hold a Sandbox's caps as the runtime's Limits ([9730d6a](https://github.com/elct9620/kobako/commit/9730d6a73fec595c2b75ac8981aeb655937045df))
* **security:** prepare an invocation's Cred as Linux's prepare_creds does ([e4f8f86](https://github.com/elct9620/kobako/commit/e4f8f86f3b25fd468f54dcc332e5767ac4ac9c32))
* **transport:** name a Call's reply CallReply beside YieldReply and CompileReply ([f34063e](https://github.com/elct9620/kobako/commit/f34063ee94edb723800b4a0670cf912aa1a445e0))
* **transport:** name the message cap by what it bounds ([133e1c1](https://github.com/elct9620/kobako/commit/133e1c151506a1f41d5f2d17ac2678648c6ed262))
* **vfs:** build a change, a call, and a device number by the names Rust and Linux use ([f5710f8](https://github.com/elct9620/kobako/commit/f5710f82f0452d8aeb7dd8ee7cc2b822ae7c14fe))
* **vfs:** read an inode's or a file's type as file_type ([48150f7](https://github.com/elct9620/kobako/commit/48150f75263e9c195a8751ba8a9654c029162aa6))
* **vfs:** spell a rename, a listing, and an open file's flags as Linux and Rust do ([5bc0175](https://github.com/elct9620/kobako/commit/5bc01757679c2b68c77e22dd86e3dac9f001f4b5))
</details>

<details><summary>kobako-wasmtime: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.20.0...kobako-wasmtime-v0.21.0) (2026-11-18)


###   BREAKING CHANGES

* **transport:** kobako_transport::envelope::Reply is CallReply.
* **transport:** call Driver::from_file in place of Driver::new;
* **security:** call kobako_runtime::security::prepare_creds in place of prepare_creds_within.
* **vfs:** call Rdir::iterate_shared and File::is_readable, is_writable, is_append, is_nonblock, is_dsync, and is_sync in place of the bare names.
* **vfs:** call Iattr::for_size, Iattr::for_times, Kiocb::new, and get_anon_bdev in place of Iattr::size, Iattr::times, Kiocb::at, and
* **vfs:** call Inode::file_type and File::file_type in place of kind.
* **runtime:** match SetupError::GuestBinaryNotFound in place of
* **network:** open_connection_limit bounds what one invocation holds open at once, not what all of a Sandbox's invocations hold together.
* **transport:** a Guest Binary built against ABI 5 is refused at construction. Rebuild it against kobako-core, whose export_guest! emits the new names. kobako-transport drops ABI_VERSION as a u32, FRAME_LEN_SIZE, MAX_FRAME_LEN, pack_ptr_len, and unpack_ptr_len, and names the ABI through ABI_VERSION ("0.6.0"), GUEST_INTERFACE, HOST_INTERFACE, DOWNCALL, UPCALL, and PREAMBLE. kobako-core drops alloc; answer returns the return area's address.
* **sdk:** `kobako::Options` and `kobako_wasmtime::Config` read and set every cap through `limits`: write `options.limits.timeout = &` where `options.timeout = &` was written, and likewise for `setup_timeout`, `memory_limit`, `stdout_limit`, `stderr_limit`, `open_file_limit`, `open_connection_limit`, and `connection_limit`.
* **options:** SandboxOptions no longer defines the DEFAULT_* constants, and SandboxOptions.new requires every option; a Sandbox's defaults are unchanged and readable through Sandbox#options.
* **runtime:** `memory_limit` now bounds the guest's `memory.grow` and what the host holds for the run together, where a Filesystem written in Ruby had its directory listings counted against a limit of their own. A run that grew close to `memory_limit` and listed a large directory may now raise `Errno::ENOMEM` in the guest or `Kobako::MemoryLimitError`, and `Usage#memory_peak` includes the listings. Raise `memory_limit` by what those listings hold to keep the earlier headroom.
* **runtime:** `kobako_runtime::FilesystemError`, `kobako_runtime::vfs::FilesystemError`, and `kobako_runtime::filesystem::FilesystemError` are now `kobako_runtime::Errno`, also reachable as `kobako::Errno`. Rename the type in a Filesystem or security module; its variants are unchanged.

### Features

* bound a guest's connections per Sandbox, per run, and per process ([7a4f869](https://github.com/elct9620/kobako/commit/7a4f8692cbc788a23c582b90112cd0275739d496))
* **io:** match patterns and change the working directory through Dir ([72cca2c](https://github.com/elct9620/kobako/commit/72cca2c400c20f6cb52d7fb0e4589fd366bf0878))
* **network:** route and refuse a Mock's dials by protocol, UDP included ([9e8b44d](https://github.com/elct9620/kobako/commit/9e8b44d425db205fe0bb2faccca0782acf1e1b4e))
* **runtime:** answer the guest's network from a table a test writes ([b3aba65](https://github.com/elct9620/kobako/commit/b3aba650760b8cff3113341500fa414b17f4dd76))
* **runtime:** make a Filesystem from its kind without wrapping it first ([e7eb8a0](https://github.com/elct9620/kobako/commit/e7eb8a00a37794226edce2aa7f1bdcab2ef6790b))
* **transport:** carry the Guest ABI in the Canonical ABI's core shape ([34b9560](https://github.com/elct9620/kobako/commit/34b956001a05e15fe29febd7098b2ceb054e1bab))
* **wasmtime:** answer a TCP socket's keep-alive-enabled from the socket ([0a78137](https://github.com/elct9620/kobako/commit/0a781371651f05196179c2c92cc22f6191e68062))
* **wasmtime:** answer a TCP socket's keepalive timing and buffer sizes ([5b248a3](https://github.com/elct9620/kobako/commit/5b248a3a58c7422c21e8a7bd31cb4f264ab5c3b6))
* **wasmtime:** answer hop limits and a UDP socket's buffer sizes ([3cf0a71](https://github.com/elct9620/kobako/commit/3cf0a71c28acfc2ac4cb7928f3e73b31b924fa90))
* **wasmtime:** answer wasi:sockets/udp with one peer a guest names ([e15500d](https://github.com/elct9620/kobako/commit/e15500d6268bb4b31c11367048731a64eaf91577))
* **wasmtime:** read the host's trust store as a Driver is built ([119ede5](https://github.com/elct9620/kobako/commit/119ede5439bb8c12523ef3607a64ee67cd45532d))
* **wasmtime:** stand the standard streams over preview2 streams ([768ba2b](https://github.com/elct9620/kobako/commit/768ba2b608e09915b3399108ffcca0d163004177))
* **wasmtime:** take the network's Protocol Family and Resolver from Config ([56cc53f](https://github.com/elct9620/kobako/commit/56cc53fa56cd987666f6b18b16e5264e314d8ef8))


### Bug Fixes

* answer an oversized datagram EMSGSIZE from the socket itself ([02317bc](https://github.com/elct9620/kobako/commit/02317bcc8e1ef86aa4b4a5e7a3b63b297cb6aeab))
* bound a guest's header list by size and charge it to its memory ([80c9ee2](https://github.com/elct9620/kobako/commit/80c9ee2523e0a3ad5de78e769a45136bcbcb6dc8))
* **core:** take no room for an empty list the host lowers ([f2fac23](https://github.com/elct9620/kobako/commit/f2fac236de645f69e479fa5bff97979005e344eb))
* **http:** answer a request whose name has no address with DNS-error ([295fe03](https://github.com/elct9620/kobako/commit/295fe036232e9256b2b3ce49f46168a51ce3f7e2))
* **http:** refuse an outgoing body finished with trailers ([c3c8f1b](https://github.com/elct9620/kobako/commit/c3c8f1b3b6277865f119c04065109a8cb4e1ab76))
* **http:** send the trailers an outgoing body finishes with ([7792bcc](https://github.com/elct9620/kobako/commit/7792bccbedff4758b2ec85bae344c45769997518))
* **io:** answer a script's socket addr at the guest's own virtual address ([6e8891a](https://github.com/elct9620/kobako/commit/6e8891a9b242849f1d1d9cea825ab47480b69b0e))
* **io:** answer each stream call with its own error, as CRuby's ([9f54696](https://github.com/elct9620/kobako/commit/9f54696635a4a0026c2555fcc6d0b6ab64b266e4))
* **io:** convert a number a file or socket call takes through to_int ([3da8767](https://github.com/elct9620/kobako/commit/3da8767e53b6c5d759b5a891357c6a3fe50ba01b))
* **io:** let the host judge a script's bind, as CRuby names it ([0ec0f80](https://github.com/elct9620/kobako/commit/0ec0f805b8a0c6caed0e4c7c25dd99c5cdaffd07))
* **io:** name every errno as CRuby does and build SystemCallError as it does ([487fcb6](https://github.com/elct9620/kobako/commit/487fcb67affc89b445296b6f482cec791f438ccd))
* **io:** raise Errno::ENFILE for a host out of open files in system ([894f8b2](https://github.com/elct9620/kobako/commit/894f8b253d29db6bddad205be45a94327e35f2e5))
* **io:** read a file's mode, names, lines, and seeks as CRuby does ([aeb0125](https://github.com/elct9620/kobako/commit/aeb01252708fecab8d5ed968866ca1b368670f0b))
* **io:** read a Float count or length the one way CRuby's NUM2LONG does ([794a529](https://github.com/elct9620/kobako/commit/794a529f3f9664f444d9c15e25964979daed429b))
* **io:** read a Float whence as the Integer it truncates to ([5386576](https://github.com/elct9620/kobako/commit/5386576297a76be3c14cc8ce87d1640cab75346e))
* **io:** read a socket stream's failure through network-error-code ([01761e1](https://github.com/elct9620/kobako/commit/01761e12d1e12484253611bb5ac2136ebff070f2))
* **io:** refuse a seek whence past C's int as CRuby's NUM2INT does ([972e3c8](https://github.com/elct9620/kobako/commit/972e3c8481eb6436b64da121dc21ad4555db933f))
* **io:** seek, putc, warn, and an unknown errno answer as CRuby's do ([86f8bff](https://github.com/elct9620/kobako/commit/86f8bffe826a01785edbdf6df142ae9353f30bac))
* judge a request's authority, path, and headers as wasmtime does ([903cc7b](https://github.com/elct9620/kobako/commit/903cc7b39217f3038de6bab5981ea37499f25e1c))
* make each guest socket and its file at creation, as socket() does ([fca4311](https://github.com/elct9620/kobako/commit/fca4311060be90df2ec79c1d21bb3a9a5b0b36ca))
* **net:** answer a host down by its own name until a WASI code groups it ([1ec6001](https://github.com/elct9620/kobako/commit/1ec60013934486bf54fe2fa2e6fb6785009d4af1))
* **net:** answer a host socket's errno as wasmtime-wasi does on every host ([293ea2c](https://github.com/elct9620/kobako/commit/293ea2cccabc6d63840e199e796af419e2687658))
* **net:** answer an IPv4-mapped address's lookup with its IPv4 address ([99f67c5](https://github.com/elct9620/kobako/commit/99f67c5da05bcc5d08222c8e505353475b68a93d))
* **net:** answer an unreachable request and a TLS alert as wasmtime-wasi-http does ([06bb9f9](https://github.com/elct9620/kobako/commit/06bb9f91c98bdb897853fc9fe0571ee20a6ee60d))
* **net:** answer EAGAIN on a shut UDP read, as non-blocking Linux does ([675376d](https://github.com/elct9620/kobako/commit/675376d971c721ca23a5bd581e7d83db59dc94aa))
* **net:** answer preview2 permits, options, and schemes as wasmtime-wasi ([3489166](https://github.com/elct9620/kobako/commit/3489166945852ca53626b82510f5c0a728cf0b26))
* **net:** bound an HTTP request by the timeouts request-options sets ([b0e35d8](https://github.com/elct9620/kobako/commit/b0e35d80a275ecb33679ff1967e05f476f155ce7))
* **net:** cancel a name lookup nobody waits for ([bf8c668](https://github.com/elct9620/kobako/commit/bf8c6682055f8993060de7c79c2d4af51a0bd111))
* **net:** deny a socket without a network as wasmtime-wasi does ([09b0e9a](https://github.com/elct9620/kobako/commit/09b0e9a2713bd76ab0aafeaa5d459566891154e6))
* **net:** hold a request's parts against memory while it is sent ([d1b642b](https://github.com/elct9620/kobako/commit/d1b642b1171d54b4c0d4cbc1ae6e8ff313b3dc89))
* **net:** hold a request's parts and a response's head against memory ([fbc93c5](https://github.com/elct9620/kobako/commit/fbc93c5d763396cb5b928801380ef3989ec931af))
* **net:** name a host socket's failure from its errno where std names none ([dddffa4](https://github.com/elct9620/kobako/commit/dddffa4bdc22575e3af4b27809662b04127aafc6))
* **net:** read a host the guest names as wasmtime-wasi's lookup does ([2defe86](https://github.com/elct9620/kobako/commit/2defe8666e9046179dab4ff8b74057e69c9ab10a))
* **net:** start a TCP socket's send buffer at Linux's default until it connects ([ea560c8](https://github.com/elct9620/kobako/commit/ea560c8ecdb0af143f94581ed10147bdc3e0359a))
* **net:** wait on a socket by its own flag, and reset a released one sent to ([9dbac06](https://github.com/elct9620/kobako/commit/9dbac065500e78c84a2ae000f06064f6f21b17f1))
* **network:** answer an endpoint the ruleset refuses as the Host App's argument ([1c6d710](https://github.com/elct9620/kobako/commit/1c6d710855495f874761eef1933f51c93a7a62b0))
* **network:** count each invocation's open connections apart ([dc426f6](https://github.com/elct9620/kobako/commit/dc426f60595b2df4ff508e7534ae37b7afed502c))
* **network:** read a Mock's names with the URL host parser ([a170e19](https://github.com/elct9620/kobako/commit/a170e197a85a1f7ca1dd6d00d62cb7dfa33b39b7))
* **network:** read a socket's own end at a virtual port, not the host's ([1ef8486](https://github.com/elct9620/kobako/commit/1ef84864b6f8106e25eaa5fd0abf7310d56835c6))
* **network:** refuse a Mock's dial with an errno that reads alike on every host ([3a7b705](https://github.com/elct9620/kobako/commit/3a7b705d82b195b4de1c54bc56fd04464dadba11))
* **network:** refuse a request whose method is not a token before any policy ([23a8e75](https://github.com/elct9620/kobako/commit/23a8e75d573b14610bdb048a0a3600017be0d69a))
* **runtime:** charge empty datagrams and virtual addresses ([9a4d9ba](https://github.com/elct9620/kobako/commit/9a4d9ba92cbd3ba5f7570f1693282202db43b6d9))
* **runtime:** charge the guest's growth and host-held memory to one budget ([f19659e](https://github.com/elct9620/kobako/commit/f19659ed93d0f5b0c1472dbc3bdbbb6815efbb4f))
* **runtime:** give a socket its virtual port as Linux's inet layer does ([2c80c02](https://github.com/elct9620/kobako/commit/2c80c027801b5a00501fb8141e8ebde53879a855))
* **runtime:** give each Invocation's outbound seam its own TLS sessions ([75119d8](https://github.com/elct9620/kobako/commit/75119d88a49f546a2d827a9476c5067d99bc02dd))
* **runtime:** give each real address its own virtual address and dial one destination ([71c1b8d](https://github.com/elct9620/kobako/commit/71c1b8d7c3ee5ee34f57e5f1a48b43619176d129))
* **runtime:** hold a socket's buffers against the Invocation's memory ([5d629a7](https://github.com/elct9620/kobako/commit/5d629a71f1940222dac36a57502030025575ef70))
* **runtime:** judge the guest's HTTP headers, binds, and peerless streams in the runtime ([bc3e024](https://github.com/elct9620/kobako/commit/bc3e024483e43d8b121cb67e661e06e99997f604))
* **runtime:** judge the request target the server reads ([9bc185e](https://github.com/elct9620/kobako/commit/9bc185e6f00375498f4d9119a34357e32d52279b))
* **runtime:** keep a deadline in a forked child with a ticker of its own ([030a3fa](https://github.com/elct9620/kobako/commit/030a3fa9360b93c0fcee4ba77c2f110ddf8d4b13))
* **runtime:** keep a failed growth in the memory peak ([2e2b986](https://github.com/elct9620/kobako/commit/2e2b986dfa47aff61f0c20ad9b63178bfccbd441))
* **runtime:** let an HTTP exchange go once nothing holds it ([2d78df9](https://github.com/elct9620/kobako/commit/2d78df98d11cdd8671629b1da2a5ac4b0b1862e7))
* **runtime:** name a host out of quota, links, or a busy program as Linux does ([6f5f816](https://github.com/elct9620/kobako/commit/6f5f816a6192dec2ea21c219a3a2f2497a6b2aac))
* **runtime:** refuse a second connect on a connecting or connected socket ([2949d40](https://github.com/elct9620/kobako/commit/2949d402303e11af2b4346a866fee754e1e00dbd))
* **runtime:** take the reactor before a TCP socket dials ([e056522](https://github.com/elct9620/kobako/commit/e0565227c357d05bca12610cd26d4aaac85b604a))
* **security:** make no Cred for a Mount that stands nowhere ([c2c6190](https://github.com/elct9620/kobako/commit/c2c619095eb452d19be1aab65941b7cb86b339dd))
* **vfs:** fail the run when a nested mount stands nowhere ([b6e44fa](https://github.com/elct9620/kobako/commit/b6e44fa43723831b2c7d37c64bbfea95f2086b83))
* **vfs:** place each mount as Linux's mount does, and open its preopen there ([a750bc7](https://github.com/elct9620/kobako/commit/a750bc75b3f1e8bcaf62ecaca1227fc0c92914c4))
* **vfs:** preopen only the outermost mounts and place the rest as Linux does ([7d21f34](https://github.com/elct9620/kobako/commit/7d21f348ab237497f4a509e464563f6c472a8a17))
* **wasmtime:** answer a connect, a short write, and a datagram's sender as wasmtime-wasi ([d1762d0](https://github.com/elct9620/kobako/commit/d1762d0a6f703c003cdb8bf00c81628f61ce2b6f))
* **wasmtime:** answer a file's preview1 calls on a socket's number as Linux does ([0ba90a0](https://github.com/elct9620/kobako/commit/0ba90a0b3369065865d6d1b5d3387217f5f37ca2))
* **wasmtime:** answer a lookup or request past the file limit ([82d9f34](https://github.com/elct9620/kobako/commit/82d9f344a1a322d3b3bb811a8bbf39b4f98ad21f))
* **wasmtime:** answer a socket's edges as wasmtime-wasi does ([2b80a61](https://github.com/elct9620/kobako/commit/2b80a61876c9eef18d8100df46b05033825b5b58))
* **wasmtime:** answer closed to a blocking flush of nothing once the send side shut ([284d5a0](https://github.com/elct9620/kobako/commit/284d5a0ba7d30ba022d27ba60ed6cc2d4fb0a3c9))
* **wasmtime:** answer new-socket-limit for a socket past the open file limit ([e45c8e8](https://github.com/elct9620/kobako/commit/e45c8e8a86003c47f69e251f0e87a77764ed2fa9))
* **wasmtime:** answer OVERFLOW for a file time a preview1 timestamp cannot hold ([a716dbf](https://github.com/elct9620/kobako/commit/a716dbf2152dc83945cdeaafc638c6f504fd3491))
* **wasmtime:** answer preview1 reads, writes, and polls on a socket's number ([5514429](https://github.com/elct9620/kobako/commit/5514429067d83e6f5775bff21a1f8ac09b70eff6))
* **wasmtime:** answer the non-blocking stream calls a guest bounds its waits with ([16ef908](https://github.com/elct9620/kobako/commit/16ef908947f5c94a37ee0f5370038936b2052e4a))
* **wasmtime:** answer the socket and stream edges as wasmtime answers them ([4f690ea](https://github.com/elct9620/kobako/commit/4f690ea29f45451982dee726c42dd178dc4ae880))
* **wasmtime:** answer three preview2 socket and stream steps as wasmtime-wasi ([702addd](https://github.com/elct9620/kobako/commit/702adddb0a2626117303bcfcc9d80eb4f0bf938e))
* **wasmtime:** answer three socket calls as wasmtime-wasi does ([e0b8c8b](https://github.com/elct9620/kobako/commit/e0b8c8b45d1b2e7b7152b6332c49b5248bd0253d))
* **wasmtime:** ask a stream for room before a blocking flush waits ([1dbf02b](https://github.com/elct9620/kobako/commit/1dbf02bdc8309344366f40245cad5a3198bcf74b))
* **wasmtime:** ask a UDP address only what wasmtime asks, and its size first ([934d2eb](https://github.com/elct9620/kobako/commit/934d2eb8b7034db263423b8ad925a56ea731b1f4))
* **wasmtime:** bound what one preview2 call lifts by its hostcall fuel ([99ebca7](https://github.com/elct9620/kobako/commit/99ebca726a803d2de0b12bfcbcf7382bf7ab1308))
* **wasmtime:** charge each preview2 handle to the Invocation's memory ([def575a](https://github.com/elct9620/kobako/commit/def575a581b777e1a4c7bcb3604d3036b01b6284))
* **wasmtime:** check a preview2 return area whole once the call returns ([9910dea](https://github.com/elct9620/kobako/commit/9910dea5e4e13056d9007b78d99f67486ebb0bdc))
* **wasmtime:** close a TCP input stream once its receive side shuts ([76d944a](https://github.com/elct9620/kobako/commit/76d944a966e152f9e16e9fc0d61b8765b65a7418))
* **wasmtime:** close an output stream once a call on it fails ([df6b96f](https://github.com/elct9620/kobako/commit/df6b96fc65e3fba8ad4bbf92edebbf35639547d2))
* **wasmtime:** count a socket, a lookup, or an exchange as one file ([b5633e4](https://github.com/elct9620/kobako/commit/b5633e4c1e2d1efba35ef4a9c3f7f97b2c1ea65e))
* **wasmtime:** derive preview1's errno from its name, and map two more to preview2 ([aabd599](https://github.com/elct9620/kobako/commit/aabd5997070291dbe5055ae6b075f85f890f674b))
* **wasmtime:** end a blocking flush on a hang-up as its pollable does ([272aeb8](https://github.com/elct9620/kobako/commit/272aeb893052954be8f2274186f6fab93a66f46e))
* **wasmtime:** end a run its host gave up at its next preview2 call ([0d1b324](https://github.com/elct9620/kobako/commit/0d1b32494c501ee346f50e39e8e38d7fb885dab9))
* **wasmtime:** give back the charge of a growth wasmtime then fails ([070c463](https://github.com/elct9620/kobako/commit/070c463e11428eb27465d6d9a9e7ba38b738249f))
* **wasmtime:** hand out only the datagrams of the peer a stream was made with ([88123ea](https://github.com/elct9620/kobako/commit/88123ea0f6f4d128ae7af8f43a9e0bf063c8c309))
* **wasmtime:** hold a socket's streams and pollables as its children ([0957eda](https://github.com/elct9620/kobako/commit/0957eda5a2159e402573ddfa7e2de5e941adea80))
* **wasmtime:** hold preview1's files in the preview2 resource table ([d447054](https://github.com/elct9620/kobako/commit/d447054dee66c3b234975ca6331dce3ed9a76d8e))
* **wasmtime:** judge a TCP socket's pollable by the connect under way as it is polled ([b89bccd](https://github.com/elct9620/kobako/commit/b89bccdba789d69dc7b69ad07a7176b0d310b963))
* **wasmtime:** lend a standard stream's handle to its descriptor ([74059ac](https://github.com/elct9620/kobako/commit/74059ac5a2651d2ffe8decc0b8feb528f49dad40))
* **wasmtime:** let a blocking flush of nothing succeed once a stream closes ([bc76ab7](https://github.com/elct9620/kobako/commit/bc76ab72fb104326cfffbd5d4d524a6d75d89887))
* **wasmtime:** make every pollable a child of what it watches ([48a0d2c](https://github.com/elct9620/kobako/commit/48a0d2c1cab25232baf29a9632e9b8a76413d236))
* **wasmtime:** number preview2 handles apart and reach no socket from preview1 ([adb0341](https://github.com/elct9620/kobako/commit/adb03410fd12804ccdaccd94748c6d1415417407))
* **wasmtime:** number preview2 handles from the descriptor space and trap a misnamed drop ([925a724](https://github.com/elct9620/kobako/commit/925a7244007e7fd72bec43f1b5dd5fa0d419dc84))
* **wasmtime:** permit a datagram send whenever its pollable is ready, and end a full flush on close ([8cf4961](https://github.com/elct9620/kobako/commit/8cf49612ee3af7ae5002ffa7b373a8b0579c2b78))
* **wasmtime:** poll an output stream ready only when it takes a write ([3247e0e](https://github.com/elct9620/kobako/commit/3247e0e80e5fee98e83e562b8fc99484d5863b8e))
* **wasmtime:** ready a closed output stream's pollable at once ([4ebd41a](https://github.com/elct9620/kobako/commit/4ebd41a90c5ee96f9e3ca90eea2fcb6246eb6ef5))
* **wasmtime:** refuse a negative read or write offset before the descriptor ([9b99512](https://github.com/elct9620/kobako/commit/9b995124380f0571b08c0cb68585c2acacf3d91a))
* **wasmtime:** refuse a short socket write whole, before any byte leaves ([5be1ec0](https://github.com/elct9620/kobako/commit/5be1ec027474376ecfcadce6d280891efe1a0afa))
* **wasmtime:** register an output stream's wait before reading its flush ([d3e9d11](https://github.com/elct9620/kobako/commit/d3e9d11ef993e18d9bbf9ceb49c0f3ec3039ec87))
* **wasmtime:** report RSYNC beside SYNC for a file opened to sync ([da46930](https://github.com/elct9620/kobako/commit/da46930be0a874bbdce2c6bccb85355a769b4912))
* **wasmtime:** reserve a producer's file once, where its limit is answered ([938ef0e](https://github.com/elct9620/kobako/commit/938ef0ea0b1531c7a5a0fe692d6b80d19dfbcc8d))
* **wasmtime:** round a keepalive time's part of a second up, as wasmtime's reaches the host ([31c5184](https://github.com/elct9620/kobako/commit/31c51843e4e6758179b05725334203915aa42611))
* **wasmtime:** spend a poll's fuel by the host's size of each subscription ([56cc303](https://github.com/elct9620/kobako/commit/56cc303bbef8f2bf03225de17d7ebab6ce384da9))
* **wasmtime:** trap a preview2 handle the guest does not hold as it is lifted ([f866c42](https://github.com/elct9620/kobako/commit/f866c4279ab54b581c0e827266c7b9ce75e4c371))
* **wasmtime:** trap and fail where the WIT says, and map errors as wasmtime-wasi does ([f8241ba](https://github.com/elct9620/kobako/commit/f8241baa1ebe9e399e6ce658f60d24e716b59522))
* **wasmtime:** trap on a misaligned preview2 return area ([6192d9e](https://github.com/elct9620/kobako/commit/6192d9e7db3d3f42ab2520b9a3a25d168bd341b6))
* **wasmtime:** trap on a preview2 list past the guest's memory before reading it ([7d9dc3c](https://github.com/elct9620/kobako/commit/7d9dc3ce63d8936053046cacaef4b31a6dadf86a))
* **wasmtime:** wait once per handle a poll lists more than once ([682fb21](https://github.com/elct9620/kobako/commit/682fb21b0623d015ebd5de4b1c78628eba99fef2))
* **wasmtime:** wait through preview2's poll from poll_oneoff ([760a280](https://github.com/elct9620/kobako/commit/760a28052853cbdb2dc9aa49cda2d1883744b86b))


### Code Refactoring

* **options:** take every option's default from the runtime ([1982afe](https://github.com/elct9620/kobako/commit/1982afe89a987a269e1d8c42f69207e05f64ee40))
* **runtime:** name the errno type Errno at the crate root ([f608e31](https://github.com/elct9620/kobako/commit/f608e31e81d1be9ddc6bb2fc8f6a26e5acafced4))
* **runtime:** name the missing Guest Binary's setup failure after what is missing ([732b8be](https://github.com/elct9620/kobako/commit/732b8bece8e796bd36d5e350b623c5ab8db28349))
* **sdk:** hold a Sandbox's caps as the runtime's Limits ([9730d6a](https://github.com/elct9620/kobako/commit/9730d6a73fec595c2b75ac8981aeb655937045df))
* **security:** prepare an invocation's Cred as Linux's prepare_creds does ([e4f8f86](https://github.com/elct9620/kobako/commit/e4f8f86f3b25fd468f54dcc332e5767ac4ac9c32))
* **transport:** load a Driver from its file, and read an origin as a lookup that may miss ([4ec8d79](https://github.com/elct9620/kobako/commit/4ec8d792ddf14bb21cfd36437b64b2389ee6df89))
* **transport:** name a Call's reply CallReply beside YieldReply and CompileReply ([f34063e](https://github.com/elct9620/kobako/commit/f34063ee94edb723800b4a0670cf912aa1a445e0))
* **vfs:** build a change, a call, and a device number by the names Rust and Linux use ([f5710f8](https://github.com/elct9620/kobako/commit/f5710f82f0452d8aeb7dd8ee7cc2b822ae7c14fe))
* **vfs:** read an inode's or a file's type as file_type ([48150f7](https://github.com/elct9620/kobako/commit/48150f75263e9c195a8751ba8a9654c029162aa6))
* **vfs:** spell a rename, a listing, and an open file's flags as Linux and Rust do ([5bc0175](https://github.com/elct9620/kobako/commit/5bc01757679c2b68c77e22dd86e3dac9f001f4b5))
</details>

<details><summary>kobako-sdk: 0.21.0</summary>

## [0.21.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.20.0...kobako-sdk-v0.21.0) (2026-11-18)


###   BREAKING CHANGES

* **transport:** kobako_transport::envelope::Reply is CallReply.
* **transport:** call Driver::from_file in place of Driver::new;
* **guest:** call Kobako::handle_id_of, Kobako::backtrace_of,
* **runtime:** match SetupError::GuestBinaryNotFound in place of
* **sdk:** write Backend { path, lifetime: Lifetime::Static(..) } in place of Backend { path, provider: Provider::Static(..) }.
* **sdk:** Sandbox::mount_with_policy and Context::mount_with_policy take (at, filesystem, access, policy); pass Access::ReadWrite for the previous read-write mount.
* **sdk:** an implementation of Receiver or ValueReceiver overrides responds_to_guest in place of respond_to_guest.
* **kobako:** Error::Argument is a struct variant, Error::Argument { kind, message }, instead of Error::Argument(String). Match Error::Argument { message, .. } where the message was matched, and Error::Argument { kind: ArgumentKind::Taken, .. } to tell one mistake from another; a match on ArgumentKind keeps a wildcard arm.
* **sdk:** `kobako::Options` and `kobako_wasmtime::Config` read and set every cap through `limits`: write `options.limits.timeout = &` where `options.timeout = &` was written, and likewise for `setup_timeout`, `memory_limit`, `stdout_limit`, `stderr_limit`, `open_file_limit`, `open_connection_limit`, and `connection_limit`.
* **sdk:** `RunPayload::bytes`, `RunPayload::build`, and `RunPayload::values` are now `RunPayload::from_bytes`, `RunPayload::from_fn`, and `RunPayload::from_values`, taking the same arguments.
* **runtime:** `kobako_runtime::FilesystemError`, `kobako_runtime::vfs::FilesystemError`, and `kobako_runtime::filesystem::FilesystemError` are now `kobako_runtime::Errno`, also reachable as `kobako::Errno`. Rename the type in a Filesystem or security module; its variants are unchanged.

### Features

* bound a guest's connections per Sandbox, per run, and per process ([7a4f869](https://github.com/elct9620/kobako/commit/7a4f8692cbc788a23c582b90112cd0275739d496))
* **kobako:** name which mistake an Error::Argument refused ([43dfd57](https://github.com/elct9620/kobako/commit/43dfd5711a8250075524fa9f1fc003102e928c76))
* **network:** route and refuse a Mock's dials by protocol, UDP included ([9e8b44d](https://github.com/elct9620/kobako/commit/9e8b44d425db205fe0bb2faccca0782acf1e1b4e))
* **runtime:** make a Filesystem from its kind without wrapping it first ([e7eb8a0](https://github.com/elct9620/kobako/commit/e7eb8a00a37794226edce2aa7f1bdcab2ef6790b))
* **sdk:** let a Rust mount with a policy keep the access it is given ([3d10278](https://github.com/elct9620/kobako/commit/3d102786f481d1e832c262b05e62616da4f8be28))
* **sdk:** take the network's backends from Options ([5b5b5d2](https://github.com/elct9620/kobako/commit/5b5b5d2b437eed5df83c8476ba4ff5eea178ea9f))
* **transport:** spell once the form of a name a guest interns ([95c10bc](https://github.com/elct9620/kobako/commit/95c10bcf7180fc0e5ef432a9935ce3f856c75c9f))


### Bug Fixes

* **codec:** charge a map's NaN keys for the comparisons they cost a host ([62eead4](https://github.com/elct9620/kobako/commit/62eead43663705a16e78e00740ed73e79a3be76f))
* **guest:** say a guest supports no block, as the glossary words an Unsupported ([3aa9a78](https://github.com/elct9620/kobako/commit/3aa9a7844780ba095fa2d197c3ab1a163a33dce6))
* **network:** answer an endpoint the ruleset refuses as the Host App's argument ([1c6d710](https://github.com/elct9620/kobako/commit/1c6d710855495f874761eef1933f51c93a7a62b0))
* **network:** read a Mock's names with the URL host parser ([a170e19](https://github.com/elct9620/kobako/commit/a170e197a85a1f7ca1dd6d00d62cb7dfa33b39b7))
* **runtime:** refuse a cap of zero in one place both frontends ask ([aef531e](https://github.com/elct9620/kobako/commit/aef531eda4f2b32bc4748b0d09b98058331af3b6))
* **sdk:** answer an unsupported yield a Receiver propagates on runtime ([ed84c1e](https://github.com/elct9620/kobako/commit/ed84c1e4910e94d25ebf90d9742c08f133cc1f74))
* **sdk:** refuse a run argument carrying a Handle the Host App wrote ([c094a9b](https://github.com/elct9620/kobako/commit/c094a9b137d8090b137f4cd12210ee23d28cfc73))
* **security:** make no Cred for a Mount that stands nowhere ([c2c6190](https://github.com/elct9620/kobako/commit/c2c619095eb452d19be1aab65941b7cb86b339dd))
* **transport:** route a Ruby dispatch before reading its arguments ([10c7635](https://github.com/elct9620/kobako/commit/10c76356d106ccd8f2da615075de93170b21a086))


### Code Refactoring

* **guest:** name the guest seams a Guest author calls by Rust's and the ABI's words ([f36c91d](https://github.com/elct9620/kobako/commit/f36c91db8bda69b5812a7982b6260b3a7d66abd6))
* **runtime:** name the errno type Errno at the crate root ([f608e31](https://github.com/elct9620/kobako/commit/f608e31e81d1be9ddc6bb2fc8f6a26e5acafced4))
* **runtime:** name the missing Guest Binary's setup failure after what is missing ([732b8be](https://github.com/elct9620/kobako/commit/732b8bece8e796bd36d5e350b623c5ab8db28349))
* **sdk:** ask a Receiver whether it responds to the guest in the third person ([f897fc2](https://github.com/elct9620/kobako/commit/f897fc2a537bf221672ee24d64cd670fda9759dd))
* **sdk:** hold a Sandbox's caps as the runtime's Limits ([9730d6a](https://github.com/elct9620/kobako/commit/9730d6a73fec595c2b75ac8981aeb655937045df))
* **sdk:** name how long an Extension backend's object lives Lifetime ([110ba8a](https://github.com/elct9620/kobako/commit/110ba8ae08aa42054ff42abd9265e28144220224))
* **sdk:** name RunPayload's constructors with from_ ([6e95532](https://github.com/elct9620/kobako/commit/6e955327f6e9d7c29641f89ac2b1edb58ed24993))
* **transport:** load a Driver from its file, and read an origin as a lookup that may miss ([4ec8d79](https://github.com/elct9620/kobako/commit/4ec8d792ddf14bb21cfd36437b64b2389ee6df89))
* **transport:** name a Call's reply CallReply beside YieldReply and CompileReply ([f34063e](https://github.com/elct9620/kobako/commit/f34063ee94edb723800b4a0670cf912aa1a445e0))
</details>

---
This PR was generated with [Release Please](https://github.com/googleapis/release-please). See [documentation](https://github.com/googleapis/release-please#release-please).
# Changelog

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

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.19.0...kobako-io-v0.20.0) (2026-11-07)


### ⚠ BREAKING CHANGES

* **io:** a path holding a NUL byte now raises ArgumentError from every File and Dir call, including the exist?, file?, and directory? questions that answered false before.

### Features

* **io:** add File::Stat and the calls built on one ([c7174e6](https://github.com/elct9620/kobako/commit/c7174e6d43ffc9aa6503d237d7e08a3e4e0df9ab))
* **io:** list, make, and remove directories through Dir ([1b862a4](https://github.com/elct9620/kobako/commit/1b862a4b9d0d7d16a287585bef8d2615d4457242))
* **io:** name EAGAIN in the guest ([2ba8d24](https://github.com/elct9620/kobako/commit/2ba8d249dda1bbec2f4ac68b708e87116a07d3b1))
* **io:** name ENODEV in the guest ([6f00e26](https://github.com/elct9620/kobako/commit/6f00e262e2f5a8d823805cf165f0d0420144df7e))
* **io:** sync, truncate, and read or write a file at an offset ([7f61b7b](https://github.com/elct9620/kobako/commit/7f61b7b26fef46e1cb5996e04a5c1d1c6ae81536))


### Bug Fixes

* **io:** answer Dir, NUL paths, pread, and advise as CRuby does ([fd9accd](https://github.com/elct9620/kobako/commit/fd9accd485d3aad2f0f36a20a292d27b81f4777b))

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.18.0...kobako-io-v0.19.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* **io:** putc, IO#read and SystemCallError.new raise RangeError for those numbers instead of writing, refusing or ignoring them.
* **guest:** the kobako guest crates name beni 0.25's types, so a shell or codec built on them moves to beni 0.25 with them.
* **guest:** the default memory_limit is now 4 MiB. JSON.parse in the guest answers an Integer of any width exactly, where it used to answer a Float past 32 bits.
* **io:** reading `IO#sync` after assigning a non-boolean now answers true or false. It used to answer the assigned value, and nil used to read back as true.
* **guest:** a Service or Extension backend bound at the path File fails its first invocation, naming the segment the Guest Binary now defines.

### Features

* **guest:** ship File in the default Guest Binary over the filesystem Grants ([535527b](https://github.com/elct9620/kobako/commit/535527b2c6beb82fe37758df62c3804ce5c7191a)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **guest:** widen the guest Integer to 64 bits ([7236a26](https://github.com/elct9620/kobako/commit/7236a26285dfdef175048f659c1094dd3bbb5b4d))
* **io:** create a file only where nothing is through File.open's wx ([56c5665](https://github.com/elct9620/kobako/commit/56c5665e4c87ac30b8035aaed207ce840efad25c))
* **io:** let IO.new take its descriptor's access when no mode is given ([77669cc](https://github.com/elct9620/kobako/commit/77669ccd958ea9518e69b50670f2804d66c9e1c5))
* **io:** name a call that would deadlock Errno::EDEADLK ([8a090e6](https://github.com/elct9620/kobako/commit/8a090e6f6949ec8019743d903c712e122d7743f9))
* **io:** name a full descriptor table Errno::EMFILE ([67876a7](https://github.com/elct9620/kobako/commit/67876a78764f8afe1b11167471d3d94e81c92af1))
* **io:** name a move across devices Errno::EXDEV ([dbed1ce](https://github.com/elct9620/kobako/commit/dbed1ce20eed6c6f1b6715f70431f0f6a7012e9e))
* **io:** name a path past its limits Errno::ENAMETOOLONG ([7679a6f](https://github.com/elct9620/kobako/commit/7679a6feb0fce0b00c89f36ed33bd5d872f86abe))
* **io:** open a file by an Integer mode of File::Constants flags ([e1f484c](https://github.com/elct9620/kobako/commit/e1f484c713df4d894744833836cf9a29ba9677ee))
* **io:** open a file through wasi-libc so a create needs no write ([6d50ca4](https://github.com/elct9620/kobako/commit/6d50ca4d79b70956d113f0798255300aa8a7fe92))
* **io:** raise Errno::EFBIG for a file grown too large ([b8ea9cd](https://github.com/elct9620/kobako/commit/b8ea9cdb6ecab7adae7f5487cffdb57889bfbbb5))
* **io:** set a file's position with IO#pos= ([1802d5a](https://github.com/elct9620/kobako/commit/1802d5a32618f33cc189e24f0de4dbfda074e3db))
* **runtime:** let a Filesystem's file refuse its close ([af16007](https://github.com/elct9620/kobako/commit/af1600797b098796dd6f329672c7d04e2e1cd2fa)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Bug Fixes

* **io:** build Errno and IOError as CRuby does ([da77c47](https://github.com/elct9620/kobako/commit/da77c4725d3701ba982f5425b1fcf2fb0038ac89))
* **io:** carry a stream's descriptor in its payload, not in hidden ivars ([76ca3c0](https://github.com/elct9620/kobako/commit/76ca3c03fdef537041a167bb58b1af2b176d772a))
* **io:** give File::Constants CRuby's Linux values ([14cadd8](https://github.com/elct9620/kobako/commit/14cadd88ac5a5dcb8f73b252b68c493488b3e78a))
* **io:** keep a stream's bookkeeping out of its instance variables ([5ecf833](https://github.com/elct9620/kobako/commit/5ecf8332c2ebff9a1d01c528fa3af31afdebdee3))
* **io:** keep the number a SystemCallError is built with ([5fd327a](https://github.com/elct9620/kobako/commit/5fd327add59173cd5756fecb41d97aebcba7b881))
* **io:** raise each code the engine answers as its own Errno ([eb6382e](https://github.com/elct9620/kobako/commit/eb6382e506a30e82576fa12ab9859e7d35be2b17))
* **io:** raise Errno::EIO for a file failure no other code names ([8a0447d](https://github.com/elct9620/kobako/commit/8a0447d2d09ea83e6f7771cf0411bfd6deec3d1a)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **io:** raise RangeError for a number past what putc, read and errno take ([6f87cc0](https://github.com/elct9620/kobako/commit/6f87cc08dff7146415f80ca25cd75ddbec4391c0))
* **io:** raise the error a failed file write names ([bcc8e00](https://github.com/elct9620/kobako/commit/bcc8e00662741fcac3b91f58c0b60f4a79d2ab9f))
* **io:** read a directory as EISDIR and wrap only an access a file holds ([df26095](https://github.com/elct9620/kobako/commit/df26095c681704cef0a8249c8b30797957fb8bd0))
* **io:** read IO.new's mode over a guest file as File.open does ([db3402c](https://github.com/elct9620/kobako/commit/db3402cd5c9c67ae48fb0363e1153a76acc55c17))
* **io:** refuse a seek before the start or by an unknown whence as EINVAL ([2f4906c](https://github.com/elct9620/kobako/commit/2f4906cc0ad3a3aa9613789e7d63c66ce881883f))
* **io:** write a file to the end when its Filesystem takes it in parts ([029b439](https://github.com/elct9620/kobako/commit/029b43939a3a48bc2925057df69bfcb85b1a0548))


### Performance Improvements

* **io:** answer IO#read with a length in one read the file holds ([e9161a2](https://github.com/elct9620/kobako/commit/e9161a2018c1617157025de275f77890e865b7d1)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Build System

* **guest:** rebuild the guest crates on beni 0.24 ([4599337](https://github.com/elct9620/kobako/commit/4599337261d1d28ff8c36435062d34cbccf09abb))
* **guest:** rebuild the guest crates on beni 0.25 ([a06dfd2](https://github.com/elct9620/kobako/commit/a06dfd25958ded9c82072149d73e56372343ba31))

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.17.0...kobako-io-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **guest:** the kobako guest crates name beni 0.23's types, so a shell or codec built on them moves to beni 0.23 with them.
* **io:** IO.new raises TypeError for a mode that is not a String, such as :w.
* **guest:** the kobako guest crates name beni 0.22's types, so a shell or codec built on them moves to beni 0.22 with them.
* **guest:** the kobako-mruby harness names beni 0.21's types — the keyword Hash in `Arguments` and `PayloadCodec::encode_call_arguments` is `beni::RHash` — so a shell or codec built on these crates moves to beni 0.21 with them.

### Features

* **guest:** rebuild the guest crates on beni 0.21 ([8ec00ff](https://github.com/elct9620/kobako/commit/8ec00ff94ec21b6bbfe8b88e0a4e75f4489dd692))


### Bug Fixes

* **io:** bracket each argument IO#write converts in an arena scope ([c1ef1b7](https://github.com/elct9620/kobako/commit/c1ef1b794bddda9347bf40436646cae8bccdd227))
* **io:** read IO.new's mode only as a String ([08954ec](https://github.com/elct9620/kobako/commit/08954ec8b14857be1f7ab10df6132e07577f802a))


### Build System

* **guest:** rebuild the guest crates on beni 0.22 ([fd6aeb9](https://github.com/elct9620/kobako/commit/fd6aeb92705fa53c52e2cd75b4918a58baa6637e))
* **guest:** rebuild the guest crates on beni 0.23 ([a6782da](https://github.com/elct9620/kobako/commit/a6782dad4c2c68a25573f82a8a16588c09856d23))

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.16.0...kobako-io-v0.17.0) (2026-09-20)


### ⚠ BREAKING CHANGES

* **guest:** a pattern's allocator is undefined from the moment the capability installs rather than from its first wrap, so allocating one is refused for the missing allocator throughout an invocation, where before the first statement could still reach an uninitialized carrier.
* **guest:** `Kobako::raise_transport_error`, `raise_service_error` and `reraise` are gone. A flow of its own builds `Kobako::transport_error` or `service_error` and hands the result back as `Err`, which beni raises at the guest call site.

### Features

* **guest:** rebuild the guest crates on beni 0.17 ([b397701](https://github.com/elct9620/kobako/commit/b397701231c7d8718503d24f73b175676d0d1be3))
* **guest:** rebuild the guest crates on beni 0.18 ([ae232eb](https://github.com/elct9620/kobako/commit/ae232ebbf708419ff0672172fbf8627363fb02da))

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.15.0...kobako-io-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.14.0...kobako-io-v0.15.0) (2026-09-13)


### Features

* **mruby:** upgrade beni to 0.14 and locate a parse failure ([586c910](https://github.com/elct9620/kobako/commit/586c910873710169c0dfce1fe7edb21f9e91cdea))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.13.1...kobako-io-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.13.0...kobako-io-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.12.0...kobako-io-v0.13.0) (2026-07-29)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.11.0...kobako-io-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.2...kobako-io-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* release the guest crates at 0.11.0 ([83391c1](https://github.com/elct9620/kobako/commit/83391c15a2bd7b162495e851ad1603a047b0cf0e))

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.1...kobako-io-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.0...kobako-io-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.9.0...kobako-io-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.8.0...kobako-io-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.7.0...kobako-io-v0.8.0) (2026-07-08)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.6.1...kobako-io-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.6.0...kobako-io-v0.6.1) (2026-07-02)


### Bug Fixes

* **guest:** size collection conversions by C array length, not #length ([90ecbd0](https://github.com/elct9620/kobako/commit/90ecbd0cb6a990b8c5a1e5deec3a10df4eaa37df))
* **io:** enforce the fd allowlist at the write syscall ([1b300df](https://github.com/elct9620/kobako/commit/1b300df7bee8f87b701f76b42300163a8899b93e))

## [0.6.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.2...kobako-io-v0.6.0) (2026-06-26)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.5.2](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.1...kobako-io-v0.5.2) (2026-06-24)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.5.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.0...kobako-io-v0.5.1) (2026-06-14)


### Bug Fixes

* **guest:** adopt beni 0.7.0 protected dispatch (B-51) ([c61655b](https://github.com/elct9620/kobako/commit/c61655bcead336d32a4b6ff7ff1b34c21cdfccd9))

## [0.5.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.4.1...kobako-io-v0.5.0) (2026-06-12)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.4.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.4.0...kobako-io-v0.4.1) (2026-06-11)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.4.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.3.0...kobako-io-v0.4.0) (2026-06-10)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## 0.3.0 (2026-06-08)


### Miscellaneous Chores

* release the guest crates at 0.3.0 ([27a0997](https://github.com/elct9620/kobako/commit/27a099766404cd9c32c54b334dc76d8ec1827675))

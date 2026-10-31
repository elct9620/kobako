:robot: I have created a release *beep* *boop*
---


<details><summary>0.30.0</summary>

## [0.30.0](https://github.com/elct9620/kobako/compare/v0.29.0...v0.30.0) (2026-10-31)


###   BREAKING CHANGES

* **runtime:** name a Filesystem's make-and-open atomic_open
* **runtime:** Overlay::with_blocking is gone, and kobako_runtime::filesystem::Blocking moved to kobako_runtime::sched::Blocking. Call kobako_runtime::sched::register_blocking once at startup instead.
* **runtime:** kobako_runtime::filesystem::Access moved to kobako_runtime::mount::Access. kobako::Access and kobako_runtime::Access are unchanged.
* **runtime:** a mount's block or Policy no longer allows writing beneath a read-only Mount, and access defaults to read. Pass access: :read_write in Ruby, or Access::ReadWrite in Rust, where the block should decide about changes. A change beneath a read-only Mount answers EROFS without asking it.
* **runtime:** File::metadata and File::set_len are gone from what an open file answers. Implement InodeOperations::getattr and setattr, answering from what the file holds where the entry must still answer once its name is gone.
* **runtime:** File::read_at is FileOperations::read_iter, and File::write_at and File::append are FileOperations::write_iter, which take a Kiocb for the offset. A write finding Kiocb::append set lands at the end and answers Unsupported if it cannot take that as one step; each moves Kiocb::pos past what it took.
* **runtime:** File::close is FileOperations::flush and takes &mut self. Move what a close reports there, and what a file lets go of to Drop.
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
* **io:** add File::Stat and the calls built on one ([c7174e6](https://github.com/elct9620/kobako/commit/c7174e6d43ffc9aa6503d237d7e08a3e4e0df9ab))
* key each dentry by the name its directory stores, as Linux's d_add_ci ([fc0267b](https://github.com/elct9620/kobako/commit/fc0267b9df310482842e0695d0350b7dc7f3a65f))
* layer an Overlay over the instance its Filesystem is mounted by ([c321548](https://github.com/elct9620/kobako/commit/c321548d294698d4b8f16336c0b035f0ee909511))
* let a mount sit inside another, as a Docker volume does ([39c843f](https://github.com/elct9620/kobako/commit/39c843f8c7b63c93433ebffd84ed46f8097e9f49))
* let no Filesystem follow a link to open a directory ([79a6a0b](https://github.com/elct9620/kobako/commit/79a6a0bbec78e273f218e863e2c22a3bb960b4de))
* mount a Filesystem instance whose Mounts share what moves ([bead3f8](https://github.com/elct9620/kobako/commit/bead3f84ebd9f4a8b240afc52aa7e0967a3a400a))
* name the Filesystems kobako ships after the Linux ones they mirror ([4fb367b](https://github.com/elct9620/kobako/commit/4fb367beefedf89ee424ff32b33837c000760f4b))
* name the instance a Mount places a Filesystem ([50ca78e](https://github.com/elct9620/kobako/commit/50ca78e405e3cd567095b49c77d3b26b2d104c57))
* reach an opened directory wherever it moves ([34522df](https://github.com/elct9620/kobako/commit/34522df97bd625f298d1a280fa73adf091879f59))
* report a file's link count and times to the guest ([5e95113](https://github.com/elct9620/kobako/commit/5e95113a056bd676bd5736f6ffa79f210a036647))
* **runtime:** answer every Filesystem call from a directory handle ([5a4376c](https://github.com/elct9620/kobako/commit/5a4376c6a8d3ede3272f8d6393ab27589f733eeb))
* **runtime:** ask a Filesystem whether a held dentry still names its entry ([13f56a4](https://github.com/elct9620/kobako/commit/13f56a40bd9622ba15ee6b34dc93393c92b3e270))
* **runtime:** bound how many Layers an Overlay stacks, as Linux does ([1c88f06](https://github.com/elct9620/kobako/commit/1c88f063d7664c9a020083df863b33b66df737c8))
* **runtime:** follow an Overlay directory through its moves ([858d5d7](https://github.com/elct9620/kobako/commit/858d5d75dbb2246475a0f3938b527a0555cc3ab9))
* **runtime:** lock an Overlay as Linux locks overlayfs, so its calls run at once ([b547dcd](https://github.com/elct9620/kobako/commit/b547dcd1abb3c33b74b0359692443e812fcb06ec))
* **runtime:** lock each directory a call changes, as Linux does ([126694b](https://github.com/elct9620/kobako/commit/126694b826c430a4b62079ddc4872873e743ef12))
* **runtime:** mark and copy up an Overlay's changes as overlayfs does ([4ea3f5f](https://github.com/elct9620/kobako/commit/4ea3f5f319d79314e1e4a80df32e6ff094bad377))
* stack an Overlay as overlayfs does, under an upper Tmpfs ([2b59780](https://github.com/elct9620/kobako/commit/2b597806809d7a810d29265e9e377d7d648d77f9))
* walk a guest path one name at a time and follow links there ([2e57fdc](https://github.com/elct9620/kobako/commit/2e57fdc11c5cbeb00697e9b287f59c15f0c91e2d))


### Bug Fixes

* ask a Ruby Filesystem by the path its directory stands at now ([c5471c0](https://github.com/elct9620/kobako/commit/c5471c0989ea085751f5c888c4c79b8ac9958cd0))
* keep an opened directory's identity through the directory itself ([28ee9e0](https://github.com/elct9620/kobako/commit/28ee9e003a30fb9159ff3f6c47dce2da25586eb2))
* open a Ruby Filesystem's file on a host whose Ruby has no NOFOLLOW ([99943cf](https://github.com/elct9620/kobako/commit/99943cf0e8abd08ff92e5f38645ea6ab111c8510))
* **regexp:** answer a condition that names a group ([faedca6](https://github.com/elct9620/kobako/commit/faedca617db4c859cd8bc9fcab3a599c6a188b22))
* **regexp:** carry capture names into a substitution block's match ([e3230b7](https://github.com/elct9620/kobako/commit/e3230b7213c2a70e14396408526a85ffa487621b))
* **regexp:** leave the match globals as MRI does after finding every match ([3d3a7f9](https://github.com/elct9620/kobako/commit/3d3a7f9673f15fd63cb810dc4bf564388fa8e24b))
* **regexp:** read a shared capture name as MRI does ([e855a5c](https://github.com/elct9620/kobako/commit/e855a5cc6588773e5852bc1b8d7230b569352dec))
* **regexp:** read inline options as MRI does ([191515b](https://github.com/elct9620/kobako/commit/191515b9cb49170156e4c78fac66c710a16e315b))
* **regexp:** read intervals as MRI does ([6eb853e](https://github.com/elct9620/kobako/commit/6eb853e6bfd69fd4a30f6f43f0df9c4f45965756))
* **regexp:** refuse a word boundary without the Unicode build ([d666e1f](https://github.com/elct9620/kobako/commit/d666e1fd54d890ffea61d9e78ae6723f7ceacd07))
* **regexp:** resolve a backreference by a shared name as MRI does ([90a7100](https://github.com/elct9620/kobako/commit/90a7100fcba1122c5f132658df68a709ecec1755))
* report the size a Ruby Filesystem's stat answers for a directory ([19211cf](https://github.com/elct9620/kobako/commit/19211cf4b2d621928be8bb098f172bec89cc759a))
* **runtime:** answer a dot path or a slash create through a link as Linux ([8aafe42](https://github.com/elct9620/kobako/commit/8aafe42cd517cdc9730ac41b092000570c065511))
* **runtime:** answer a link call by what stands, as filename_linkat does ([5a0ac25](https://github.com/elct9620/kobako/commit/5a0ac257a260f8638caf794df50d95ad11fb139d))
* **runtime:** answer each path call in Linux's order ([85d0407](https://github.com/elct9620/kobako/commit/85d040761699ca72d3a1e2ec5737d9f9ccfcbd47))
* **runtime:** ask an entry about itself however a guest reached it, as fstat and ftruncate do ([1166426](https://github.com/elct9620/kobako/commit/1166426d3f4bc27d9c2d0d3a8f2b7f1a6a23f59e))
* **runtime:** ask the readlink hook only of a link ([d584cbb](https://github.com/elct9620/kobako/commit/d584cbb66d34672f49cb44a0226071277a029275))
* **runtime:** bound the host directories a Bind keeps open ([531cf3a](https://github.com/elct9620/kobako/commit/531cf3a6f84bc706d00a9eb91bdafb4f76962f6e))
* **runtime:** climb an Overlay's directories in a loop ([70706f3](https://github.com/elct9620/kobako/commit/70706f34d121e335d64cc05fe255cd67f67af614))
* **runtime:** copy a file up before setting its times through an Overlay ([1a20b59](https://github.com/elct9620/kobako/commit/1a20b59c0db975ebe2bc36f7d5a051b64f325940))
* **runtime:** empty a file an open truncates through setattr ([0ee006f](https://github.com/elct9620/kobako/commit/0ee006f4683ed3059a9f3251d450bf3de0eb365d))
* **runtime:** find and create an O_CREAT open's file under one directory lock ([a3684d4](https://github.com/elct9620/kobako/commit/a3684d4ca79e7b8ae518118adf9ec965ba46ba9a))
* **runtime:** hold a Bind's directory descriptor to what its dentry recorded ([f9e7fc9](https://github.com/elct9620/kobako/commit/f9e7fc99ab37a7f41eb67661bbf7c8c1466ff090))
* **runtime:** hold an Overlay directory's Layers instead of reaching them again ([85d5539](https://github.com/elct9620/kobako/commit/85d5539ee0ad6c98e0b20e376f540fcaf704ac16))
* **runtime:** judge a time set through a descriptor where it stands ([e1e5147](https://github.com/elct9620/kobako/commit/e1e51475c63ea20593b28b86fdc59fce7e51e019))
* **runtime:** keep pipes among the Filesystems, reading nothing into no room ([79a2240](https://github.com/elct9620/kobako/commit/79a22401f9c8ab3c8fe777955a4b39e880418686))
* **runtime:** keep the flags a directory was opened with ([e328aa4](https://github.com/elct9620/kobako/commit/e328aa4d6ef59aaef970d3b9e162f5edeb9d6222))
* **runtime:** key the inode number a permissive guest reads ([1d2b8b2](https://github.com/elct9620/kobako/commit/1d2b8b276c805aba557b336bae6cca31fd330570))
* **runtime:** leave a move between two names of one entry alone ([717d321](https://github.com/elct9620/kobako/commit/717d3214ffc7a0a11527898a1b0c067f6d53b42d))
* **runtime:** leave the descriptor table to the Driver, as FD_ADD reserves first ([12e6b65](https://github.com/elct9620/kobako/commit/12e6b65b4b47bdd4e73465f904c9cef92c27e1bd))
* **runtime:** let a directory answer as one open to read does ([3919be4](https://github.com/elct9620/kobako/commit/3919be4a542fc0d63fcc18422c3465b06fd6c6d1))
* **runtime:** let a file open to read follow its copy up in an Overlay ([10b55f5](https://github.com/elct9620/kobako/commit/10b55f5ba95bfec2de62749917b9ab933836581c))
* **runtime:** let a link leading out escape as a `..` does ([47d7ec7](https://github.com/elct9620/kobako/commit/47d7ec7dfa97bd1d3d1a9bfee0c536372e42108d))
* **runtime:** let a listing hand its own dots and answer for a removed directory ([88a3fc7](https://github.com/elct9620/kobako/commit/88a3fc7e491206b23a3eab2b4c91bf9aa0f00786))
* **runtime:** let a permissive guest tell entries apart by inode number ([e651562](https://github.com/elct9620/kobako/commit/e651562d40fd55028415975f4305f6f3f512ca28))
* **runtime:** let a Policy only refuse more than a Mount's access allows ([f1502b5](https://github.com/elct9620/kobako/commit/f1502b522bcb9d8fc8fffd29a7714b5d777bb793))
* **runtime:** let a Ruby file open report the entry it stands for ([a7bc682](https://github.com/elct9620/kobako/commit/a7bc6821b7d2286079e7b6a1262bfa7a539f3771))
* **runtime:** let an entry's setattr decide whether it keeps a time the guest sets ([fdfa3fb](https://github.com/elct9620/kobako/commit/fdfa3fb197909c1f027db5e5c540bc5cbfd5f013))
* **runtime:** let an open file answer for its mode and the largest offset ([a737685](https://github.com/elct9620/kobako/commit/a7376851ba17aa5e9c84a5158424d2ce5d367eaf))
* **runtime:** let go of a deep Tmpfs or dentry in a loop ([13456e3](https://github.com/elct9620/kobako/commit/13456e368580f31a451bfc78be59b631c56569c2))
* **runtime:** let no Filesystem open follow a link ([09166ac](https://github.com/elct9620/kobako/commit/09166ac45f2b447f9c7e203794163ebed6c991e4))
* **runtime:** merge only directory over directory in an Overlay ([6473956](https://github.com/elct9620/kobako/commit/6473956fd773a858ea34880b6a1d7b8e60018f3f))
* **runtime:** move seeking, flags, hints, allocation and readiness into File ([5d1cd6b](https://github.com/elct9620/kobako/commit/5d1cd6b1d1d626945b5f5c70a5342a8576a017f8))
* **runtime:** open only the entry a call judged on a Filesystem reached by path ([6a650e5](https://github.com/elct9620/kobako/commit/6a650e5f92a4ca06234e613b5dac8f3714de5e89))
* **runtime:** refuse one instance as two Layers of an Overlay ([b2c2206](https://github.com/elct9620/kobako/commit/b2c22060f9eceefb2e6a6048bde3960aa4d11dc9))
* **runtime:** refuse removing a path ending in .. as ENOTEMPTY, as Linux does ([a02bac9](https://github.com/elct9620/kobako/commit/a02bac9e9ea300013e90ab396226fe03fbf45f1d))
* **runtime:** refuse to open a link before its Filesystem is asked ([54b0349](https://github.com/elct9620/kobako/commit/54b03493790711056dd051e4492800e444a7d997))
* **runtime:** remove and move through an Overlay's upper as Linux's VFS helpers do ([7973488](https://github.com/elct9620/kobako/commit/79734884adc44c840376b10a0f3dcbec146f6830))
* **runtime:** report the kind a host gives a special file ([b421805](https://github.com/elct9620/kobako/commit/b421805dc4b76cc76f541d1b9e0579329e72a502))
* **runtime:** take O_SYNC and O_DIRECTORY's other flags as Linux does ([ba6bc57](https://github.com/elct9620/kobako/commit/ba6bc57e1ad8264e9035d9808098efe5b846f32f))
* **wasmtime:** let a directory renamed onto its own path stay ([5fa99ab](https://github.com/elct9620/kobako/commit/5fa99ab7052cf4dbbd6be34bc2d79351d42e6311))
* **wasmtime:** refuse a link to an absolute path through preview1 ([3477e84](https://github.com/elct9620/kobako/commit/3477e840cd15ac4bd6f06105e4ee9842717a05bc))


### Performance Improvements

* **runtime:** ask an Overlay's Layers about each name once ([2528559](https://github.com/elct9620/kobako/commit/25285596d8f2a64bca18f16d6a5f7bfaea6e4d0d))
* **runtime:** ask an Overlay's lowest Layer for no markers ([e101335](https://github.com/elct9620/kobako/commit/e101335e53be004359c2851d7723a2bdd9155d5d))
* **runtime:** keep names found standing for the next walk ([51dfee4](https://github.com/elct9620/kobako/commit/51dfee488898f130cb0efdb7cb59d1b7a0a38957))
* **runtime:** open the directories a path walks through in one call ([453ad84](https://github.com/elct9620/kobako/commit/453ad84fed5f84b9f16fcf6148482e90c3280844))
* **runtime:** skip the whiteout lookups an Overlay already answered ([69aaefc](https://github.com/elct9620/kobako/commit/69aaefcc74bcf039a65abcbc6c6e113d29098eb1))
* **runtime:** walk a directory one Overlay Layer shows as that Layer does ([e93dd53](https://github.com/elct9620/kobako/commit/e93dd53a42404d304a94da9b6ea89a5b5f06f009))


### Code Refactoring

* **runtime:** answer a close through flush, as close(2) reads f_op-&gt;flush ([c34eb58](https://github.com/elct9620/kobako/commit/c34eb58b297e5be6ef249af03c6d7655cfd94729))
* **runtime:** hang each entry's operations on its inode, as Linux does ([e21470b](https://github.com/elct9620/kobako/commit/e21470b625255eb85b4b5ff868872f2db5a7967e))
* **runtime:** land each read and write through a Kiocb, as read_iter and write_iter ([bc35dfd](https://github.com/elct9620/kobako/commit/bc35dfd13085b6aee517748996647bd7083cdbc8))
* **runtime:** let the mount layer own what a Guest may do beneath a Mount ([0dcde21](https://github.com/elct9620/kobako/commit/0dcde2143d7935a8e4b205bb03d664974af7edfb))
* **runtime:** move how a thread waits for a lock beneath the VFS ([616f1c8](https://github.com/elct9620/kobako/commit/616f1c82bd946c3501727e6e5e17d842f5b20ae7))
* **runtime:** name a Filesystem's make-and-open atomic_open ([57a6e10](https://github.com/elct9620/kobako/commit/57a6e10cb14e7a7f6ff02d2a7a88b5e18e459165))
* **runtime:** name what an open file answers as Linux's file_operations ([ca835e9](https://github.com/elct9620/kobako/commit/ca835e99b821845bcfa56648ff7336c67242bbd7))
</details>

<details><summary>kobako-core: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.19.0...kobako-core-v0.20.0) (2026-10-31)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions
</details>

<details><summary>kobako-mruby: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.19.0...kobako-mruby-v0.20.0) (2026-10-31)


### Miscellaneous Chores

* **kobako-mruby:** Synchronize kobako crates versions
</details>

<details><summary>kobako-io: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.19.0...kobako-io-v0.20.0) (2026-10-31)


### Features

* **io:** add File::Stat and the calls built on one ([c7174e6](https://github.com/elct9620/kobako/commit/c7174e6d43ffc9aa6503d237d7e08a3e4e0df9ab))
</details>

<details><summary>kobako-regexp: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.19.0...kobako-regexp-v0.20.0) (2026-10-31)


###   BREAKING CHANGES

* **regexp:** These patterns now raise RegexpError where they used to answer. Remove the backreference, look-ahead or atomic group from a look-behind, and use `(?:...)` for a group inside a negative look-behind. Replace `(?s)` with `(?m)`, the language's spelling of the same option, and drop `(?U)`, `(?R)` and any other option outside `(?imxdau-imx)`. Rename a group whose name opens with a digit, and order an interval's bounds as `{lower,upper}`.
* **regexp:** `/(?<n>a)(b)/` no longer captures `(b)`, so `to_a`, `size`, `captures` and `$1` change; name the group to keep it. `\1`, `\k<1>`, `\g<1>` and `(?(1)...)` beside a named group now raise RegexpError; refer by name instead. `(?P<n>...)`, `(?P=n)` and `(?P>n)` now raise RegexpError; write `(?<n>...)`, `\k<n>` and `\g<n>`.

### Bug Fixes

* **regexp:** answer a condition that names a group ([faedca6](https://github.com/elct9620/kobako/commit/faedca617db4c859cd8bc9fcab3a599c6a188b22))
* **regexp:** carry capture names into a substitution block's match ([e3230b7](https://github.com/elct9620/kobako/commit/e3230b7213c2a70e14396408526a85ffa487621b))
* **regexp:** leave the match globals as MRI does after finding every match ([3d3a7f9](https://github.com/elct9620/kobako/commit/3d3a7f9673f15fd63cb810dc4bf564388fa8e24b))
* **regexp:** read a pattern's groups as MRI does beside a named group ([15f6742](https://github.com/elct9620/kobako/commit/15f6742a141c506ef013623f79062a3d8a84964f))
* **regexp:** read a shared capture name as MRI does ([e855a5c](https://github.com/elct9620/kobako/commit/e855a5cc6588773e5852bc1b8d7230b569352dec))
* **regexp:** read inline options as MRI does ([191515b](https://github.com/elct9620/kobako/commit/191515b9cb49170156e4c78fac66c710a16e315b))
* **regexp:** read intervals as MRI does ([6eb853e](https://github.com/elct9620/kobako/commit/6eb853e6bfd69fd4a30f6f43f0df9c4f45965756))
* **regexp:** refuse a word boundary without the Unicode build ([d666e1f](https://github.com/elct9620/kobako/commit/d666e1fd54d890ffea61d9e78ae6723f7ceacd07))
* **regexp:** refuse the syntax MRI refuses ([da26615](https://github.com/elct9620/kobako/commit/da26615b7aa8378767574a7b579aea1d79b75bd9))
* **regexp:** resolve a backreference by a shared name as MRI does ([90a7100](https://github.com/elct9620/kobako/commit/90a7100fcba1122c5f132658df68a709ecec1755))
</details>

<details><summary>kobako-json: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.19.0...kobako-json-v0.20.0) (2026-10-31)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions
</details>

<details><summary>kobako-baker: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.19.0...kobako-baker-v0.20.0) (2026-10-31)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions
</details>

<details><summary>kobako-transport: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.19.0...kobako-transport-v0.20.0) (2026-10-31)


### Miscellaneous Chores

* **kobako-transport:** Synchronize kobako crates versions
</details>

<details><summary>kobako-codec: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.19.0...kobako-codec-v0.20.0) (2026-10-31)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions
</details>

<details><summary>kobako-runtime: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.19.0...kobako-runtime-v0.20.0) (2026-10-31)


###   BREAKING CHANGES

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
* **runtime:** bound how many Layers an Overlay stacks, as Linux does ([1c88f06](https://github.com/elct9620/kobako/commit/1c88f063d7664c9a020083df863b33b66df737c8))
* **runtime:** follow an Overlay directory through its moves ([858d5d7](https://github.com/elct9620/kobako/commit/858d5d75dbb2246475a0f3938b527a0555cc3ab9))
* **runtime:** lock an Overlay as Linux locks overlayfs, so its calls run at once ([b547dcd](https://github.com/elct9620/kobako/commit/b547dcd1abb3c33b74b0359692443e812fcb06ec))
* **runtime:** lock each directory a call changes, as Linux does ([126694b](https://github.com/elct9620/kobako/commit/126694b826c430a4b62079ddc4872873e743ef12))
* **runtime:** mark and copy up an Overlay's changes as overlayfs does ([4ea3f5f](https://github.com/elct9620/kobako/commit/4ea3f5f319d79314e1e4a80df32e6ff094bad377))
* stack an Overlay as overlayfs does, under an upper Tmpfs ([2b59780](https://github.com/elct9620/kobako/commit/2b597806809d7a810d29265e9e377d7d648d77f9))
* walk a guest path one name at a time and follow links there ([2e57fdc](https://github.com/elct9620/kobako/commit/2e57fdc11c5cbeb00697e9b287f59c15f0c91e2d))


### Bug Fixes

* keep an opened directory's identity through the directory itself ([28ee9e0](https://github.com/elct9620/kobako/commit/28ee9e003a30fb9159ff3f6c47dce2da25586eb2))
* **runtime:** align an Overlay's answers with overlayfs's order ([ccfb7d4](https://github.com/elct9620/kobako/commit/ccfb7d46c2d4889cc837746f3a82345f27dde8b1))
* **runtime:** answer a Bind dentry stale once its host moved what it named ([cb94c51](https://github.com/elct9620/kobako/commit/cb94c514f8568d619fc3e8d97efb798a6649568c))
* **runtime:** answer a dot path or a slash create through a link as Linux ([8aafe42](https://github.com/elct9620/kobako/commit/8aafe42cd517cdc9730ac41b092000570c065511))
* **runtime:** answer a link call by what stands, as filename_linkat does ([5a0ac25](https://github.com/elct9620/kobako/commit/5a0ac257a260f8638caf794df50d95ad11fb139d))
* **runtime:** answer about a name through DentryOperations ([ae6ca8a](https://github.com/elct9620/kobako/commit/ae6ca8abaebdbfd289378ba9299a6446bb611e02))
* **runtime:** answer an Overlay file from where its data stands now ([024f274](https://github.com/elct9620/kobako/commit/024f274904b9fb0a9c09a05a4b5d40e1558b65a3))
* **runtime:** answer each path call in Linux's order ([85d0407](https://github.com/elct9620/kobako/commit/85d040761699ca72d3a1e2ec5737d9f9ccfcbd47))
* **runtime:** answer what a Filesystem says when a directory will not open ([5ef653c](https://github.com/elct9620/kobako/commit/5ef653c35f222fbd0e78c906551b2880b4d62f7d))
* **runtime:** ask an entry about itself however a guest reached it, as fstat and ftruncate do ([1166426](https://github.com/elct9620/kobako/commit/1166426d3f4bc27d9c2d0d3a8f2b7f1a6a23f59e))
* **runtime:** ask the readlink hook only of a link ([d584cbb](https://github.com/elct9620/kobako/commit/d584cbb66d34672f49cb44a0226071277a029275))
* **runtime:** bind an Overlay entry's Layers as its lookup finds them ([b76c5a2](https://github.com/elct9620/kobako/commit/b76c5a226059299f97bf607b7d4e0796e4b232ec))
* **runtime:** bound the host directories a Bind keeps open ([531cf3a](https://github.com/elct9620/kobako/commit/531cf3a6f84bc706d00a9eb91bdafb4f76962f6e))
* **runtime:** climb an Overlay's directories in a loop ([70706f3](https://github.com/elct9620/kobako/commit/70706f34d121e335d64cc05fe255cd67f67af614))
* **runtime:** copy a file up before setting its times through an Overlay ([1a20b59](https://github.com/elct9620/kobako/commit/1a20b59c0db975ebe2bc36f7d5a051b64f325940))
* **runtime:** copy up from what an Overlay file's inode keeps ([ce9abb5](https://github.com/elct9620/kobako/commit/ce9abb52e1a6402305dad5214a3a321bdc615bd9))
* **runtime:** count no names for a Tmpfs file nothing holds any more ([20c96be](https://github.com/elct9620/kobako/commit/20c96bec2534d005b84e931025ded3cf6be69c08))
* **runtime:** empty a file an open truncates through setattr ([0ee006f](https://github.com/elct9620/kobako/commit/0ee006f4683ed3059a9f3251d450bf3de0eb365d))
* **runtime:** find a move's names once both directories are held ([4803a12](https://github.com/elct9620/kobako/commit/4803a12216770e6e348ed33dc6711334aeb6ca9b))
* **runtime:** find and create an O_CREAT open's file under one directory lock ([a3684d4](https://github.com/elct9620/kobako/commit/a3684d4ca79e7b8ae518118adf9ec965ba46ba9a))
* **runtime:** hold a Bind's directory descriptor to what its dentry recorded ([f9e7fc9](https://github.com/elct9620/kobako/commit/f9e7fc99ab37a7f41eb67661bbf7c8c1466ff090))
* **runtime:** hold a directory the guest opens from the moment it opens ([1dcca08](https://github.com/elct9620/kobako/commit/1dcca089ffc5ce8459940565999ca09b20bee44a))
* **runtime:** hold an Overlay directory's Layers instead of reaching them again ([85d5539](https://github.com/elct9620/kobako/commit/85d5539ee0ad6c98e0b20e376f540fcaf704ac16))
* **runtime:** judge a time set through a descriptor where it stands ([e1e5147](https://github.com/elct9620/kobako/commit/e1e51475c63ea20593b28b86fdc59fce7e51e019))
* **runtime:** keep a Bind directory's host descriptor on its dentry, walked as 9p walks ([8ce61ef](https://github.com/elct9620/kobako/commit/8ce61ef074d194db0e250e88c98a5afceab27152))
* **runtime:** keep an Overlay's view of a directory it emptied ([e869247](https://github.com/elct9620/kobako/commit/e8692472ed50690882ee85cc41816cd60f6003be))
* **runtime:** keep both files of a Tmpfs exchange named ([fa50c7c](https://github.com/elct9620/kobako/commit/fa50c7c6fde0c4e0cd382d358457e53212eab9d4))
* **runtime:** keep pipes among the Filesystems, reading nothing into no room ([79a2240](https://github.com/elct9620/kobako/commit/79a22401f9c8ab3c8fe777955a4b39e880418686))
* **runtime:** keep the flags a directory was opened with ([e328aa4](https://github.com/elct9620/kobako/commit/e328aa4d6ef59aaef970d3b9e162f5edeb9d6222))
* **runtime:** key the inode number a permissive guest reads ([1d2b8b2](https://github.com/elct9620/kobako/commit/1d2b8b276c805aba557b336bae6cca31fd330570))
* **runtime:** learn what every looked-up name names, so a moved Bind directory is never reached by its old path ([773498a](https://github.com/elct9620/kobako/commit/773498a0b7dac76b30d482d48d82cc5a1da5e811))
* **runtime:** leave a move between two names of one entry alone ([717d321](https://github.com/elct9620/kobako/commit/717d3214ffc7a0a11527898a1b0c067f6d53b42d))
* **runtime:** leave the descriptor table to the Driver, as FD_ADD reserves first ([12e6b65](https://github.com/elct9620/kobako/commit/12e6b65b4b47bdd4e73465f904c9cef92c27e1bd))
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
* **runtime:** let go of a deep Tmpfs or dentry in a loop ([13456e3](https://github.com/elct9620/kobako/commit/13456e368580f31a451bfc78be59b631c56569c2))
* **runtime:** let no Filesystem open follow a link ([09166ac](https://github.com/elct9620/kobako/commit/09166ac45f2b447f9c7e203794163ebed6c991e4))
* **runtime:** merge only directory over directory in an Overlay ([6473956](https://github.com/elct9620/kobako/commit/6473956fd773a858ea34880b6a1d7b8e60018f3f))
* **runtime:** move seeking, flags, hints, allocation and readiness into File ([5d1cd6b](https://github.com/elct9620/kobako/commit/5d1cd6b1d1d626945b5f5c70a5342a8576a017f8))
* **runtime:** open a standard stream as a pipe's end, as create_pipe_files does ([58b3f68](https://github.com/elct9620/kobako/commit/58b3f686db485bd39f44e69bca25675acdad215a))
* **runtime:** open only the entry a call judged on a Filesystem reached by path ([6a650e5](https://github.com/elct9620/kobako/commit/6a650e5f92a4ca06234e613b5dac8f3714de5e89))
* **runtime:** record what a name names in the Filesystem's own lookup, as -&gt;lookup calls d_add ([fab0cfd](https://github.com/elct9620/kobako/commit/fab0cfd24f3eab1d4b52a8a547fda3518644b34d))
* **runtime:** refuse a listed name no directory can hold ([e6f4232](https://github.com/elct9620/kobako/commit/e6f4232f3df014b2f49cbff2b9756222d0bdf8ff))
* **runtime:** refuse a name Windows reads as another on a Bind ([f8ca1bb](https://github.com/elct9620/kobako/commit/f8ca1bbe6c199cdc39e185ba70229d4976d85822))
* **runtime:** refuse a path no call can take in the VFS ([e752648](https://github.com/elct9620/kobako/commit/e7526485f91ca2acbfe59ad4431d2385c91caa19))
* **runtime:** refuse a spelling a Bind's host lists under no name as EACCES ([bc053d4](https://github.com/elct9620/kobako/commit/bc053d401be0fe409be0dbb00d6ecbef7f33db27))
* **runtime:** refuse a sync of a file without one, as vfs_fsync_range does ([0f26730](https://github.com/elct9620/kobako/commit/0f267304ef9966fb797194817ccfc21134389570))
* **runtime:** refuse one instance as two Layers of an Overlay ([b2c2206](https://github.com/elct9620/kobako/commit/b2c22060f9eceefb2e6a6048bde3960aa4d11dc9))
* **runtime:** refuse removing a path ending in .. as ENOTEMPTY, as Linux does ([a02bac9](https://github.com/elct9620/kobako/commit/a02bac9e9ea300013e90ab396226fe03fbf45f1d))
* **runtime:** refuse to open a link before its Filesystem is asked ([54b0349](https://github.com/elct9620/kobako/commit/54b03493790711056dd051e4492800e444a7d997))
* **runtime:** remove and move through an Overlay's upper as Linux's VFS helpers do ([7973488](https://github.com/elct9620/kobako/commit/79734884adc44c840376b10a0f3dcbec146f6830))
* **runtime:** report the kind a host gives a special file ([b421805](https://github.com/elct9620/kobako/commit/b421805dc4b76cc76f541d1b9e0579329e72a502))
* **runtime:** settle a path's last name only after the walk steps into a directory ([7616d08](https://github.com/elct9620/kobako/commit/7616d0800a51f4ac76d39911c9c0ff4c5e83422b))
* **runtime:** size a Tmpfs or Bind directory as Linux does instead of zero ([ed84e7f](https://github.com/elct9620/kobako/commit/ed84e7fdec91e4c10b683b9a75ff2ebc9eaa8c66))
* **runtime:** swap the two dentries an exchange swaps, as Linux does ([b0ca503](https://github.com/elct9620/kobako/commit/b0ca503b74c3d8d659dddb8bfeac471af19e1b89))
* **runtime:** take O_SYNC and O_DIRECTORY's other flags as Linux does ([ba6bc57](https://github.com/elct9620/kobako/commit/ba6bc57e1ad8264e9035d9808098efe5b846f32f))
* **runtime:** walk a dentry's parents under the rename seqlock, as Linux does ([8df90f0](https://github.com/elct9620/kobako/commit/8df90f08bcbf80d59a3c2075bc97f6cad40c69f4))
* **runtime:** walk a path's last name as Linux's namei does ([591435b](https://github.com/elct9620/kobako/commit/591435b5abedf30c62c00fc634bbafe7f1dc68fc))


### Performance Improvements

* **runtime:** ask an Overlay's Layers about each name once ([2528559](https://github.com/elct9620/kobako/commit/25285596d8f2a64bca18f16d6a5f7bfaea6e4d0d))
* **runtime:** ask an Overlay's lowest Layer for no markers ([e101335](https://github.com/elct9620/kobako/commit/e101335e53be004359c2851d7723a2bdd9155d5d))
* **runtime:** keep names found standing for the next walk ([51dfee4](https://github.com/elct9620/kobako/commit/51dfee488898f130cb0efdb7cb59d1b7a0a38957))
* **runtime:** open the directories a path walks through in one call ([453ad84](https://github.com/elct9620/kobako/commit/453ad84fed5f84b9f16fcf6148482e90c3280844))
* **runtime:** skip the whiteout lookups an Overlay already answered ([69aaefc](https://github.com/elct9620/kobako/commit/69aaefcc74bcf039a65abcbc6c6e113d29098eb1))
* **runtime:** spell an exactly matching Linux name without asking for it ([b499a8c](https://github.com/elct9620/kobako/commit/b499a8cbe145f6e7504fa55a389be181e188763c))
* **runtime:** spell exactly matching Linux names without listing ([3cb8172](https://github.com/elct9620/kobako/commit/3cb81720daa48a90844afad3168070338a43d0e3))
* **runtime:** walk a directory one Overlay Layer shows as that Layer does ([e93dd53](https://github.com/elct9620/kobako/commit/e93dd53a42404d304a94da9b6ea89a5b5f06f009))


### Code Refactoring

* **runtime:** answer a close through flush, as close(2) reads f_op-&gt;flush ([c34eb58](https://github.com/elct9620/kobako/commit/c34eb58b297e5be6ef249af03c6d7655cfd94729))
* **runtime:** drop the question a Mount no longer has anyone asking ([6cc6249](https://github.com/elct9620/kobako/commit/6cc6249c0b1e47d3aa9f0f86168c991650c38ea2))
* **runtime:** hang each entry's operations on its inode, as Linux does ([e21470b](https://github.com/elct9620/kobako/commit/e21470b625255eb85b4b5ff868872f2db5a7967e))
* **runtime:** land each read and write through a Kiocb, as read_iter and write_iter ([bc35dfd](https://github.com/elct9620/kobako/commit/bc35dfd13085b6aee517748996647bd7083cdbc8))
* **runtime:** let the mount layer own what a Guest may do beneath a Mount ([0dcde21](https://github.com/elct9620/kobako/commit/0dcde2143d7935a8e4b205bb03d664974af7edfb))
* **runtime:** move how a thread waits for a lock beneath the VFS ([616f1c8](https://github.com/elct9620/kobako/commit/616f1c82bd946c3501727e6e5e17d842f5b20ae7))
* **runtime:** name a Filesystem's make-and-open atomic_open ([57a6e10](https://github.com/elct9620/kobako/commit/57a6e10cb14e7a7f6ff02d2a7a88b5e18e459165))
* **runtime:** name what an open file answers as Linux's file_operations ([ca835e9](https://github.com/elct9620/kobako/commit/ca835e99b821845bcfa56648ff7336c67242bbd7))
</details>

<details><summary>kobako-wasmtime: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.19.0...kobako-wasmtime-v0.20.0) (2026-10-31)


###   BREAKING CHANGES

* **runtime:** name a Filesystem's make-and-open atomic_open
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
* Overlay::new() and Overlay::layer are replaced by Overlay::new(lower, upper), which refuses an upper that is no Tmpfs. In Ruby, Overlay.new.layer(fs, access:) is replaced by Overlay.new(lower: [...], upper: tmpfs).
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
* reach an opened directory wherever it moves ([34522df](https://github.com/elct9620/kobako/commit/34522df97bd625f298d1a280fa73adf091879f59))
* report a file's link count and times to the guest ([5e95113](https://github.com/elct9620/kobako/commit/5e95113a056bd676bd5736f6ffa79f210a036647))
* **runtime:** answer every Filesystem call from a directory handle ([5a4376c](https://github.com/elct9620/kobako/commit/5a4376c6a8d3ede3272f8d6393ab27589f733eeb))
* **runtime:** ask a Filesystem whether a held dentry still names its entry ([13f56a4](https://github.com/elct9620/kobako/commit/13f56a40bd9622ba15ee6b34dc93393c92b3e270))
* stack an Overlay as overlayfs does, under an upper Tmpfs ([2b59780](https://github.com/elct9620/kobako/commit/2b597806809d7a810d29265e9e377d7d648d77f9))
* walk a guest path one name at a time and follow links there ([2e57fdc](https://github.com/elct9620/kobako/commit/2e57fdc11c5cbeb00697e9b287f59c15f0c91e2d))


### Bug Fixes

* keep an opened directory's identity through the directory itself ([28ee9e0](https://github.com/elct9620/kobako/commit/28ee9e003a30fb9159ff3f6c47dce2da25586eb2))
* **runtime:** answer a dot path or a slash create through a link as Linux ([8aafe42](https://github.com/elct9620/kobako/commit/8aafe42cd517cdc9730ac41b092000570c065511))
* **runtime:** answer a link call by what stands, as filename_linkat does ([5a0ac25](https://github.com/elct9620/kobako/commit/5a0ac257a260f8638caf794df50d95ad11fb139d))
* **runtime:** answer about a name through DentryOperations ([ae6ca8a](https://github.com/elct9620/kobako/commit/ae6ca8abaebdbfd289378ba9299a6446bb611e02))
* **runtime:** ask an entry about itself however a guest reached it, as fstat and ftruncate do ([1166426](https://github.com/elct9620/kobako/commit/1166426d3f4bc27d9c2d0d3a8f2b7f1a6a23f59e))
* **runtime:** empty a file an open truncates through setattr ([0ee006f](https://github.com/elct9620/kobako/commit/0ee006f4683ed3059a9f3251d450bf3de0eb365d))
* **runtime:** hold a directory the guest opens from the moment it opens ([1dcca08](https://github.com/elct9620/kobako/commit/1dcca089ffc5ce8459940565999ca09b20bee44a))
* **runtime:** keep pipes among the Filesystems, reading nothing into no room ([79a2240](https://github.com/elct9620/kobako/commit/79a22401f9c8ab3c8fe777955a4b39e880418686))
* **runtime:** leave the descriptor table to the Driver, as FD_ADD reserves first ([12e6b65](https://github.com/elct9620/kobako/commit/12e6b65b4b47bdd4e73465f904c9cef92c27e1bd))
* **runtime:** let a directory answer as one open to read does ([3919be4](https://github.com/elct9620/kobako/commit/3919be4a542fc0d63fcc18422c3465b06fd6c6d1))
* **runtime:** let a link leading out escape as a `..` does ([47d7ec7](https://github.com/elct9620/kobako/commit/47d7ec7dfa97bd1d3d1a9bfee0c536372e42108d))
* **runtime:** let a listing hand its own dots and answer for a removed directory ([88a3fc7](https://github.com/elct9620/kobako/commit/88a3fc7e491206b23a3eab2b4c91bf9aa0f00786))
* **runtime:** let a permissive guest tell entries apart by inode number ([e651562](https://github.com/elct9620/kobako/commit/e651562d40fd55028415975f4305f6f3f512ca28))
* **runtime:** let a Policy only refuse more than a Mount's access allows ([f1502b5](https://github.com/elct9620/kobako/commit/f1502b522bcb9d8fc8fffd29a7714b5d777bb793))
* **runtime:** let an entry's setattr decide whether it keeps a time the guest sets ([fdfa3fb](https://github.com/elct9620/kobako/commit/fdfa3fb197909c1f027db5e5c540bc5cbfd5f013))
* **runtime:** let an open file answer for its mode and the largest offset ([a737685](https://github.com/elct9620/kobako/commit/a7376851ba17aa5e9c84a5158424d2ce5d367eaf))
* **runtime:** merge only directory over directory in an Overlay ([6473956](https://github.com/elct9620/kobako/commit/6473956fd773a858ea34880b6a1d7b8e60018f3f))
* **runtime:** move seeking, flags, hints, allocation and readiness into File ([5d1cd6b](https://github.com/elct9620/kobako/commit/5d1cd6b1d1d626945b5f5c70a5342a8576a017f8))
* **runtime:** open a standard stream as a pipe's end, as create_pipe_files does ([58b3f68](https://github.com/elct9620/kobako/commit/58b3f686db485bd39f44e69bca25675acdad215a))
* **runtime:** refuse a path no call can take in the VFS ([e752648](https://github.com/elct9620/kobako/commit/e7526485f91ca2acbfe59ad4431d2385c91caa19))
* **runtime:** refuse a sync of a file without one, as vfs_fsync_range does ([0f26730](https://github.com/elct9620/kobako/commit/0f267304ef9966fb797194817ccfc21134389570))
* **runtime:** refuse removing a path ending in .. as ENOTEMPTY, as Linux does ([a02bac9](https://github.com/elct9620/kobako/commit/a02bac9e9ea300013e90ab396226fe03fbf45f1d))
* **runtime:** report the kind a host gives a special file ([b421805](https://github.com/elct9620/kobako/commit/b421805dc4b76cc76f541d1b9e0579329e72a502))
* **runtime:** take O_SYNC and O_DIRECTORY's other flags as Linux does ([ba6bc57](https://github.com/elct9620/kobako/commit/ba6bc57e1ad8264e9035d9808098efe5b846f32f))
* **runtime:** walk a path's last name as Linux's namei does ([591435b](https://github.com/elct9620/kobako/commit/591435b5abedf30c62c00fc634bbafe7f1dc68fc))
* **wasmtime:** let a directory renamed onto its own path stay ([5fa99ab](https://github.com/elct9620/kobako/commit/5fa99ab7052cf4dbbd6be34bc2d79351d42e6311))
* **wasmtime:** refuse a link to an absolute path through preview1 ([3477e84](https://github.com/elct9620/kobako/commit/3477e840cd15ac4bd6f06105e4ee9842717a05bc))


### Code Refactoring

* **runtime:** answer a close through flush, as close(2) reads f_op-&gt;flush ([c34eb58](https://github.com/elct9620/kobako/commit/c34eb58b297e5be6ef249af03c6d7655cfd94729))
* **runtime:** hang each entry's operations on its inode, as Linux does ([e21470b](https://github.com/elct9620/kobako/commit/e21470b625255eb85b4b5ff868872f2db5a7967e))
* **runtime:** land each read and write through a Kiocb, as read_iter and write_iter ([bc35dfd](https://github.com/elct9620/kobako/commit/bc35dfd13085b6aee517748996647bd7083cdbc8))
* **runtime:** let the mount layer own what a Guest may do beneath a Mount ([0dcde21](https://github.com/elct9620/kobako/commit/0dcde2143d7935a8e4b205bb03d664974af7edfb))
* **runtime:** name a Filesystem's make-and-open atomic_open ([57a6e10](https://github.com/elct9620/kobako/commit/57a6e10cb14e7a7f6ff02d2a7a88b5e18e459165))
* **runtime:** name what an open file answers as Linux's file_operations ([ca835e9](https://github.com/elct9620/kobako/commit/ca835e99b821845bcfa56648ff7336c67242bbd7))
</details>

<details><summary>kobako-sdk: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.19.0...kobako-sdk-v0.20.0) (2026-10-31)


###   BREAKING CHANGES

* **runtime:** kobako_runtime::filesystem::Access moved to kobako_runtime::mount::Access. kobako::Access and kobako_runtime::Access are unchanged.
* **runtime:** a mount's block or Policy no longer allows writing beneath a read-only Mount, and access defaults to read. Pass access: :read_write in Ruby, or Access::ReadWrite in Rust, where the block should decide about changes. A change beneath a read-only Mount answers EROFS without asking it.
* **runtime:** File::metadata and File::set_len are gone from what an open file answers. Implement InodeOperations::getattr and setattr, answering from what the file holds where the entry must still answer once its name is gone.
* **runtime:** File::read_at is FileOperations::read_iter, and File::write_at and File::append are FileOperations::write_iter, which take a Kiocb for the offset. A write finding Kiocb::append set lands at the end and answers Unsupported if it cannot take that as one step; each moves Kiocb::pos past what it took.
* **runtime:** the File trait is FileOperations. Rename it where a Filesystem implements it.
* **runtime:** the Filesystem trait is InodeOperations, whatever earlier notes call it. Each entry answers through its own: a lookup returns a Lookup, whose Inode holds them, and an operation an entry lacks answers None.
* Inode::stored_name is replaced by Inode::lookup, which the runtime asks before keeping a name's dentry. A rename that changes only a name's case on a host folding case leaves it as it was, as a rename of an entry onto itself does.
* Inode::rename takes the source, the target directory, and the target, without a Rename; Rename and Rename::check are removed. A Filesystem written in Ruby answers rename(from, to) instead of rename(from, to, directory:).
* a Filesystem is the instance a Mount places, made with Filesystem::new from its root Inode. Mount::new, the Rust Sandbox's and Context's mount and mount_with_policy, and Overlay::layer take one, and
* Kobako::Filesystem::Memory is Kobako::Filesystem::Tmpfs with size: and nr_inodes: in place of limit: and inodes:, and Kobako::Filesystem::Directory is Kobako::Filesystem::Bind. In Rust, Memory is Tmpfs with with_size, with_nr_inodes, and DEFAULT_SIZE; Directory is Bind; and the Filesystem trait is Inode.
* the Filesystem trait's open, open_dir, symlink_metadata, stored_name, read_link, create_dir, remove_dir, remove_file, and rename take a &Dentry instead of a name; open_dir_nofollow is now open_dir, and metadata() has no default. The kobako crate re-exports Dentry.
* the Filesystem trait's metadata(path) is removed; metadata() reports the directory itself and defaults to what symlink_metadata answers for the empty path.
* the Filesystem trait's open_dir is removed; open_dir_nofollow opens a directory by name and refuses a link.
* Mount::new, and the Rust Sandbox's and Context's mount and mount_with_policy, take a shared instance made from the Filesystem instead of the Filesystem itself.
* the Filesystem trait's canonicalize_beneath and for_policy are removed and read_link is added; a Filesystem holding no links answers read_link with InvalidInput. The Mount's for_invocation is removed, and a Directory no longer has a mode following no link.
* the Filesystem trait's hold_directory and the Hold type are removed; a directory a Filesystem opens holds what it needs open itself. Beneath a Policy, a directory opened on a Filesystem that reports no inode number now answers ENOENT.
* **runtime:** a Filesystem implementation answers open_dir, open_dir_nofollow, entries and stored_name in place of read_dir and stored_path, and its rename takes the target directory before the target path.
* Kobako::Filesystem::Temporary and the Rust Temporary are removed; mount a Memory, and mount a new one through the Context where each run should start empty. A Filesystem implementation drops its for_invocation method.

### Features

* ask a Filesystem about a directory itself, never through a link ([5881b87](https://github.com/elct9620/kobako/commit/5881b87d8e06cdabd12fe958c65f5bc67b24375d))
* check what a rename moves in the VFS, as Linux does ([dd5e267](https://github.com/elct9620/kobako/commit/dd5e267b7d3f09591864575ce8b16aaf2f703a07))
* hand a Filesystem the dentry the VFS made for each name ([5a2f85a](https://github.com/elct9620/kobako/commit/5a2f85a030918f329b98874af843385ea5ba12a8))
* hand every invocation the Filesystem its Mount holds ([17bb576](https://github.com/elct9620/kobako/commit/17bb576803f8366a073993efa17df788949f8a69))
* key each dentry by the name its directory stores, as Linux's d_add_ci ([fc0267b](https://github.com/elct9620/kobako/commit/fc0267b9df310482842e0695d0350b7dc7f3a65f))
* let a mount sit inside another, as a Docker volume does ([39c843f](https://github.com/elct9620/kobako/commit/39c843f8c7b63c93433ebffd84ed46f8097e9f49))
* let no Filesystem follow a link to open a directory ([79a6a0b](https://github.com/elct9620/kobako/commit/79a6a0bbec78e273f218e863e2c22a3bb960b4de))
* mount a Filesystem instance whose Mounts share what moves ([bead3f8](https://github.com/elct9620/kobako/commit/bead3f84ebd9f4a8b240afc52aa7e0967a3a400a))
* name the Filesystems kobako ships after the Linux ones they mirror ([4fb367b](https://github.com/elct9620/kobako/commit/4fb367beefedf89ee424ff32b33837c000760f4b))
* name the instance a Mount places a Filesystem ([50ca78e](https://github.com/elct9620/kobako/commit/50ca78e405e3cd567095b49c77d3b26b2d104c57))
* **runtime:** answer every Filesystem call from a directory handle ([5a4376c](https://github.com/elct9620/kobako/commit/5a4376c6a8d3ede3272f8d6393ab27589f733eeb))
* **runtime:** ask a Filesystem whether a held dentry still names its entry ([13f56a4](https://github.com/elct9620/kobako/commit/13f56a40bd9622ba15ee6b34dc93393c92b3e270))
* walk a guest path one name at a time and follow links there ([2e57fdc](https://github.com/elct9620/kobako/commit/2e57fdc11c5cbeb00697e9b287f59c15f0c91e2d))


### Bug Fixes

* keep an opened directory's identity through the directory itself ([28ee9e0](https://github.com/elct9620/kobako/commit/28ee9e003a30fb9159ff3f6c47dce2da25586eb2))
* **runtime:** ask an entry about itself however a guest reached it, as fstat and ftruncate do ([1166426](https://github.com/elct9620/kobako/commit/1166426d3f4bc27d9c2d0d3a8f2b7f1a6a23f59e))
* **runtime:** let a Policy only refuse more than a Mount's access allows ([f1502b5](https://github.com/elct9620/kobako/commit/f1502b522bcb9d8fc8fffd29a7714b5d777bb793))


### Code Refactoring

* **runtime:** hang each entry's operations on its inode, as Linux does ([e21470b](https://github.com/elct9620/kobako/commit/e21470b625255eb85b4b5ff868872f2db5a7967e))
* **runtime:** land each read and write through a Kiocb, as read_iter and write_iter ([bc35dfd](https://github.com/elct9620/kobako/commit/bc35dfd13085b6aee517748996647bd7083cdbc8))
* **runtime:** let the mount layer own what a Guest may do beneath a Mount ([0dcde21](https://github.com/elct9620/kobako/commit/0dcde2143d7935a8e4b205bb03d664974af7edfb))
* **runtime:** name what an open file answers as Linux's file_operations ([ca835e9](https://github.com/elct9620/kobako/commit/ca835e99b821845bcfa56648ff7336c67242bbd7))
</details>

---
This PR was generated with [Release Please](https://github.com/googleapis/release-please). See [documentation](https://github.com/googleapis/release-please#release-please).
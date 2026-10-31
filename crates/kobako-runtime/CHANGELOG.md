# Changelog

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.19.0...kobako-runtime-v0.20.0) (2026-10-31)


### ⚠ BREAKING CHANGES

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

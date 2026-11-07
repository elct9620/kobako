# Changelog

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.19.0...kobako-wasmtime-v0.20.0) (2026-11-07)


### ⚠ BREAKING CHANGES

* **io:** a path holding a NUL byte now raises ArgumentError from every File and Dir call, including the exist?, file?, and directory? questions that answered false before.
* **runtime:** Policy::allows is asked Access::ReadWrite before an open that may create finds what stands, so an open of an existing file asking to create is asked ReadWrite and then the access it opens for, where it was asked that access alone.
* **runtime:** Kstat's len, accessed, modified, and changed fields are now size, atime, mtime, and ctime, and Profile::guest_metadata is now
* **runtime:** spell each file type as Linux's FT_* names it
* **runtime:** name what a path reports Kstat, as Linux's struct kstat
* **runtime:** build an open's flags once and hand them to do_file_open
* **runtime:** answer EAGAIN to a `..` a move landed before
* **runtime:** judge a path through a link where the link leads
* **runtime:** kobako_runtime::Denial is gone. A Denial::ReadOnly is now
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
* **runtime:** judge a path through a link where the link leads ([7407a71](https://github.com/elct9620/kobako/commit/7407a71cfbcb9c4f5faa3ef954c592d0687174cc))
* **runtime:** keep the times a caller sets on a Tmpfs, a Bind, and an Overlay ([d53d547](https://github.com/elct9620/kobako/commit/d53d5479c48e4d423addf02b7d373784409d1604))
* stack an Overlay as overlayfs does, under an upper Tmpfs ([2b59780](https://github.com/elct9620/kobako/commit/2b597806809d7a810d29265e9e377d7d648d77f9))
* walk a guest path one name at a time and follow links there ([2e57fdc](https://github.com/elct9620/kobako/commit/2e57fdc11c5cbeb00697e9b287f59c15f0c91e2d))
* **wasmtime:** judge every invocation by the security modules a Driver is built with ([ba42b85](https://github.com/elct9620/kobako/commit/ba42b85f43ba8eb4bacba807a4c5af1788a614ae))


### Bug Fixes

* **io:** answer Dir, NUL paths, pread, and advise as CRuby does ([fd9accd](https://github.com/elct9620/kobako/commit/fd9accd485d3aad2f0f36a20a292d27b81f4777b))
* keep an opened directory's identity through the directory itself ([28ee9e0](https://github.com/elct9620/kobako/commit/28ee9e003a30fb9159ff3f6c47dce2da25586eb2))
* **runtime:** answer a dot path or a slash create through a link as Linux ([8aafe42](https://github.com/elct9620/kobako/commit/8aafe42cd517cdc9730ac41b092000570c065511))
* **runtime:** answer a link call by what stands, as filename_linkat does ([5a0ac25](https://github.com/elct9620/kobako/commit/5a0ac257a260f8638caf794df50d95ad11fb139d))
* **runtime:** answer a move's flags and a link's source where Linux does ([be7af48](https://github.com/elct9620/kobako/commit/be7af48aa4dce202f9a2d1342a3cd0e314240cd4))
* **runtime:** answer about a name through DentryOperations ([ae6ca8a](https://github.com/elct9620/kobako/commit/ae6ca8abaebdbfd289378ba9299a6446bb611e02))
* **runtime:** answer EAGAIN to a `..` a move landed before ([1aa66dd](https://github.com/elct9620/kobako/commit/1aa66dda78bd63aff91a78bfe8d6bff6306a9c19))
* **runtime:** answer ENODEV to an allocation on a device ([0ba099b](https://github.com/elct9620/kobako/commit/0ba099b474890179e3c4c4e8a13837b9be5b5ead))
* **runtime:** ask a walk's start whether it still stands under its directory's lock ([4b14e4e](https://github.com/elct9620/kobako/commit/4b14e4e91aaf1a91d2815789ed41c2da9c50e97d))
* **runtime:** ask an entry about itself however a guest reached it, as fstat and ftruncate do ([1166426](https://github.com/elct9620/kobako/commit/1166426d3f4bc27d9c2d0d3a8f2b7f1a6a23f59e))
* **runtime:** ask atomic_open for an open's last name no walk looked up ([9860ae9](https://github.com/elct9620/kobako/commit/9860ae9391874cf27bf63345b4672458f696a8a8))
* **runtime:** ask to make a file before an open finds what stands ([0b3ecf5](https://github.com/elct9620/kobako/commit/0b3ecf54c6975576dee13decb1904ccb371f30be))
* **runtime:** build an open's flags once and hand them to do_file_open ([6672734](https://github.com/elct9620/kobako/commit/6672734fc42af8c2040402c8ed2b14d562c31328))
* **runtime:** empty a file an open truncates through setattr ([0ee006f](https://github.com/elct9620/kobako/commit/0ee006f4683ed3059a9f3251d450bf3de0eb365d))
* **runtime:** hand a listed entry where it stands, as dir_emit does ([804f8b2](https://github.com/elct9620/kobako/commit/804f8b2ce93a8d5b935d400d295ba5f2ce7c5a32))
* **runtime:** hold a directory the guest opens from the moment it opens ([1dcca08](https://github.com/elct9620/kobako/commit/1dcca089ffc5ce8459940565999ca09b20bee44a))
* **runtime:** hold what a guest's listings keep within its memory limit ([e4d74a5](https://github.com/elct9620/kobako/commit/e4d74a5ad2d5554e1b509ee606c1911811f66be8))
* **runtime:** keep a Bind's walk and allocation answers true under a race ([cce1a2a](https://github.com/elct9620/kobako/commit/cce1a2a0e64bf44648e01caf14e901f1cb04fad3))
* **runtime:** keep a Cred without kobako's rules inside the runtime ([775bb5c](https://github.com/elct9620/kobako/commit/775bb5c09c1fb8ce4412208df8113e26260e89c9))
* **runtime:** keep pipes among the Filesystems, reading nothing into no room ([79a2240](https://github.com/elct9620/kobako/commit/79a22401f9c8ab3c8fe777955a4b39e880418686))
* **runtime:** leave the descriptor table to the Driver, as FD_ADD reserves first ([12e6b65](https://github.com/elct9620/kobako/commit/12e6b65b4b47bdd4e73465f904c9cef92c27e1bd))
* **runtime:** let a descriptor's stat and listing ask no hook ([48d265a](https://github.com/elct9620/kobako/commit/48d265ab83773e1ee436f06d4f822bf57f372ba2))
* **runtime:** let a directory answer as one open to read does ([3919be4](https://github.com/elct9620/kobako/commit/3919be4a542fc0d63fcc18422c3465b06fd6c6d1))
* **runtime:** let a link leading out escape as a `..` does ([47d7ec7](https://github.com/elct9620/kobako/commit/47d7ec7dfa97bd1d3d1a9bfee0c536372e42108d))
* **runtime:** let a listing hand its own dots and answer for a removed directory ([88a3fc7](https://github.com/elct9620/kobako/commit/88a3fc7e491206b23a3eab2b4c91bf9aa0f00786))
* **runtime:** let a permissive guest tell entries apart by inode number ([e651562](https://github.com/elct9620/kobako/commit/e651562d40fd55028415975f4305f6f3f512ca28))
* **runtime:** let a Policy only refuse more than a Mount's access allows ([f1502b5](https://github.com/elct9620/kobako/commit/f1502b522bcb9d8fc8fffd29a7714b5d777bb793))
* **runtime:** let an entry's setattr decide whether it keeps a time the guest sets ([fdfa3fb](https://github.com/elct9620/kobako/commit/fdfa3fb197909c1f027db5e5c540bc5cbfd5f013))
* **runtime:** let an open file answer for its mode and the largest offset ([a737685](https://github.com/elct9620/kobako/commit/a7376851ba17aa5e9c84a5158424d2ce5d367eaf))
* **runtime:** list a directory as its Filesystem says, through dir_emit ([c7447a1](https://github.com/elct9620/kobako/commit/c7447a137a5237b2e071ba6500fddd736abe5092))
* **runtime:** merge only directory over directory in an Overlay ([6473956](https://github.com/elct9620/kobako/commit/6473956fd773a858ea34880b6a1d7b8e60018f3f))
* **runtime:** move seeking, flags, hints, allocation and readiness into File ([5d1cd6b](https://github.com/elct9620/kobako/commit/5d1cd6b1d1d626945b5f5c70a5342a8576a017f8))
* **runtime:** name a listing's dots as Linux's drivers do ([ef26d61](https://github.com/elct9620/kobako/commit/ef26d61318ad3f489f5d1379db690854d46a2900))
* **runtime:** open a file made by create alone after the security modules ([5f8d30c](https://github.com/elct9620/kobako/commit/5f8d30c48e08450fa3b1065954fad8ea8e8e5a26))
* **runtime:** open a made file once its directory is unlocked ([1a61088](https://github.com/elct9620/kobako/commit/1a61088b4adbbeaef052e83ad6d8ac19b79d7523))
* **runtime:** open a standard stream as a pipe's end, as create_pipe_files does ([58b3f68](https://github.com/elct9620/kobako/commit/58b3f686db485bd39f44e69bca25675acdad215a))
* **runtime:** open each Mount's root before its Policy applies ([4580697](https://github.com/elct9620/kobako/commit/45806973ab0b99f977b7d90253f8dafa9b1f8e8b))
* **runtime:** read a call's flags and path before where it starts ([70bce0f](https://github.com/elct9620/kobako/commit/70bce0f0d5b282e1324ab042d5c29b2e523cb0ae))
* **runtime:** read a link at an empty path from the descriptor itself ([c59f6c2](https://github.com/elct9620/kobako/commit/c59f6c2a9c69d40ec08afd961bc6f71ea66cc020))
* **runtime:** refuse a change at an Overlay's super block when it has no upper ([7f4c31e](https://github.com/elct9620/kobako/commit/7f4c31e8db91d09fc6a133708e14f28a26d60675))
* **runtime:** refuse a path no call can take in the VFS ([e752648](https://github.com/elct9620/kobako/commit/e7526485f91ca2acbfe59ad4431d2385c91caa19))
* **runtime:** refuse a sync of a file without one, as vfs_fsync_range does ([0f26730](https://github.com/elct9620/kobako/commit/0f267304ef9966fb797194817ccfc21134389570))
* **runtime:** refuse an advice from past the largest signed offset ([3ff54a2](https://github.com/elct9620/kobako/commit/3ff54a2f0859c694f012a2ab228295050a500e76))
* **runtime:** refuse removing a path ending in .. as ENOTEMPTY, as Linux does ([a02bac9](https://github.com/elct9620/kobako/commit/a02bac9e9ea300013e90ab396226fe03fbf45f1d))
* **runtime:** report the kind a host gives a special file ([b421805](https://github.com/elct9620/kobako/commit/b421805dc4b76cc76f541d1b9e0579329e72a502))
* **runtime:** reserve the room a guest allocates in a Tmpfs and a Bind ([dd94317](https://github.com/elct9620/kobako/commit/dd94317257c3bbbbfd2a2a170a9d5a3783801f43))
* **runtime:** sync only as far as asked, cap one call, and let a writer through ([5e2902f](https://github.com/elct9620/kobako/commit/5e2902f95312c985f78053cacf6374db893e174f))
* **runtime:** take O_SYNC and O_DIRECTORY's other flags as Linux does ([ba6bc57](https://github.com/elct9620/kobako/commit/ba6bc57e1ad8264e9035d9808098efe5b846f32f))
* **runtime:** walk a path's last name as Linux's namei does ([591435b](https://github.com/elct9620/kobako/commit/591435b5abedf30c62c00fc634bbafe7f1dc68fc))
* **runtime:** walk out of a link and create through one as Linux does ([c6e4054](https://github.com/elct9620/kobako/commit/c6e40547e52421bb7f8a1f86e549c40f365002ad))
* **wasmtime:** answer a call setting no time before its descriptor ([31a58a3](https://github.com/elct9620/kobako/commit/31a58a3ccd541c27120c31d5f42fcaee4be5201d))
* **wasmtime:** answer a path call's flags and name before its descriptor ([d8771ff](https://github.com/elct9620/kobako/commit/d8771ff42a967c04452e74a1f1f35d47ecf0b985))
* **wasmtime:** answer a path call's names and descriptors in Linux's order ([a2f2db4](https://github.com/elct9620/kobako/commit/a2f2db47be863037f904367af9a5cc9750e0111d))
* **wasmtime:** answer the entries a listing wrote before it failed ([67364af](https://github.com/elct9620/kobako/commit/67364afb2bed53a9b3685e299a82d8a61801a4e9))
* **wasmtime:** answer what a listing wrote before a name it stops at ([89974ff](https://github.com/elct9620/kobako/commit/89974fff2fc5fefad99ce8ff26befe91e92130aa))
* **wasmtime:** encode ESTALE as preview1's stale ([625371e](https://github.com/elct9620/kobako/commit/625371e961c370e73f49aae771ed3b9f5253d51b))
* **wasmtime:** hand down the right to write from a directory a guest opens ([c3e2297](https://github.com/elct9620/kobako/commit/c3e229780ecb666f18b7c9b9d85c35bb14cb96c3))
* **wasmtime:** let a directory renamed onto its own path stay ([5fa99ab](https://github.com/elct9620/kobako/commit/5fa99ab7052cf4dbbd6be34bc2d79351d42e6311))
* **wasmtime:** make a path call again a few times on EAGAIN ([a47b33e](https://github.com/elct9620/kobako/commit/a47b33e39c24eb77b193cfef7f57ab3d1cd17790))
* **wasmtime:** make the start's check part of the call made again ([858643a](https://github.com/elct9620/kobako/commit/858643a471d372df99b0eb92bb49d4d8d78fef7d))
* **wasmtime:** move a renumbered descriptor before closing what it replaces ([bcf5f46](https://github.com/elct9620/kobako/commit/bcf5f464566788b41aca771287908bfd01bf347e))
* **wasmtime:** open to read a file made or emptied without a right ([7e86943](https://github.com/elct9620/kobako/commit/7e869434bd93db4760ee6c6a04edbd6b8bb3a622))
* **wasmtime:** refuse a descriptor that stands only for its path as fdget does ([52be6ed](https://github.com/elct9620/kobako/commit/52be6edc630fe8db5537d0910a9a28eb2324c0ee))
* **wasmtime:** refuse a link to an absolute path through preview1 ([3477e84](https://github.com/elct9620/kobako/commit/3477e840cd15ac4bd6f06105e4ee9842717a05bc))
* **wasmtime:** refuse a readlink into no room before its path ([37f8c13](https://github.com/elct9620/kobako/commit/37f8c1360920712490a9c86409cc427560b7f5c3))
* **wasmtime:** report a descriptor's kind from its open file ([18c4c7e](https://github.com/elct9620/kobako/commit/18c4c7eecc96a8588574ad8a80853c31d93466fc))
* **wasmtime:** report a stream's flags and rights as it holds them ([cfa3475](https://github.com/elct9620/kobako/commit/cfa3475c65be7ba730fb0bb6e6e490c22078c544))
* **wasmtime:** report a wait on a descriptor not open in its event ([479b545](https://github.com/elct9620/kobako/commit/479b5459940ae62645ee62737363383679e22571))
* **wasmtime:** reserve an open's descriptor before reading its name ([a85dfe6](https://github.com/elct9620/kobako/commit/a85dfe65763a55c6993f5420c3b76cf068ee693e))
* **wasmtime:** stop a guest's waits for a lock at its deadline ([e102796](https://github.com/elct9620/kobako/commit/e102796b34b63098f3cce0050fb5ed1f16349b94))
* **wasmtime:** take a socket call's descriptor as fdget does ([5e5205f](https://github.com/elct9620/kobako/commit/5e5205fd3403f1cbbd167a9e87879f88797d8421))


### Code Refactoring

* **runtime:** answer a close through flush, as close(2) reads f_op-&gt;flush ([c34eb58](https://github.com/elct9620/kobako/commit/c34eb58b297e5be6ef249af03c6d7655cfd94729))
* **runtime:** answer every VFS call with an errno ([84a0a38](https://github.com/elct9620/kobako/commit/84a0a385149f5a9194fcab9598849cf7c92c46ae))
* **runtime:** hang each entry's operations on its inode, as Linux does ([e21470b](https://github.com/elct9620/kobako/commit/e21470b625255eb85b4b5ff868872f2db5a7967e))
* **runtime:** land each read and write through a Kiocb, as read_iter and write_iter ([bc35dfd](https://github.com/elct9620/kobako/commit/bc35dfd13085b6aee517748996647bd7083cdbc8))
* **runtime:** let the mount layer own what a Guest may do beneath a Mount ([0dcde21](https://github.com/elct9620/kobako/commit/0dcde2143d7935a8e4b205bb03d664974af7edfb))
* **runtime:** name a Filesystem's make-and-open atomic_open ([57a6e10](https://github.com/elct9620/kobako/commit/57a6e10cb14e7a7f6ff02d2a7a88b5e18e459165))
* **runtime:** name what a path reports Kstat, as Linux's struct kstat ([0f43bed](https://github.com/elct9620/kobako/commit/0f43bed4242c2236e02c88d00470d1d31180219e))
* **runtime:** name what an open file answers as Linux's file_operations ([ca835e9](https://github.com/elct9620/kobako/commit/ca835e99b821845bcfa56648ff7336c67242bbd7))
* **runtime:** spell a Kstat's fields as Linux's struct kstat does ([ec73cac](https://github.com/elct9620/kobako/commit/ec73cac38e9b7e8b9162ea59ebc35de7e9394c8d))
* **runtime:** spell each file type as Linux's FT_* names it ([796fac4](https://github.com/elct9620/kobako/commit/796fac412cae9c70b6f80e7f33946fe960ed520e))

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.18.0...kobako-wasmtime-v0.19.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* **wasmtime:** a struct literal no longer builds the wasmtime driver's Config outside its crate; start from `Config::default()` and assign each field.
* **runtime:** a struct literal no longer builds OpenOptions, Metadata, DirEntry, Snapshot, Capture, or Usage outside kobako-runtime; use their `new` or `Default` and assign each field.
* **wasmtime:** a Service answer past 16 MiB now raises a Service failure in the guest where it used to raise the transport error, and an answer the host cannot write into guest memory now traps the invocation.
* **wasmtime:** a Guest Binary stalling while an invocation sets it up ends in the deadline's trap, and a cap tripped there keeps its own kind, where it used to hang or trap as a generic failure.
* the Guest ABI is version 5, so a Guest Binary built against version 4 is refused at Sandbox construction until rebuilt. In kobako-transport, Reply gains a Break arm and YieldReply::Break carries no payload. The Yielder trait of kobako-runtime gains frame and takes the frame in yield_to_block; the Guest trait of kobako-core and the MrbGuest trait of kobako-mruby take the frame in yield_to_block; Downcall::Yield carries frame and args.
* **io:** putc, IO#read and SystemCallError.new raise RangeError for those numbers instead of writing, refusing or ignoring them.
* **guest:** the default memory_limit is now 4 MiB. JSON.parse in the guest answers an Integer of any width exactly, where it used to answer a Float past 32 bits.
* **guest:** a Service or Extension backend bound at the path File fails its first invocation, naming the segment the Guest Binary now defines.
* **runtime:** the Runtime trait's invoke takes the invocation's Grants as a fourth argument, so every engine implementing it must accept them.

### Features

* check what a rename moves as it moves, refusing a directory beneath a Policy ([23794bf](https://github.com/elct9620/kobako/commit/23794bfcb54e148126e29873fa9b52d9e3de8d8c))
* **guest:** carry mruby-bigint so an Integer has no width of its own ([a74266d](https://github.com/elct9620/kobako/commit/a74266d3e7b1295b75770b62894b232195f9b3bb))
* **guest:** ship File in the default Guest Binary over the filesystem Grants ([535527b](https://github.com/elct9620/kobako/commit/535527b2c6beb82fe37758df62c3804ce5c7191a)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **guest:** widen the guest Integer to 64 bits ([7236a26](https://github.com/elct9620/kobako/commit/7236a26285dfdef175048f659c1094dd3bbb5b4d))
* **io:** create a file only where nothing is through File.open's wx ([56c5665](https://github.com/elct9620/kobako/commit/56c5665e4c87ac30b8035aaed207ce840efad25c))
* **io:** open a file by an Integer mode of File::Constants flags ([e1f484c](https://github.com/elct9620/kobako/commit/e1f484c713df4d894744833836cf9a29ba9677ee))
* **io:** open a file through wasi-libc so a create needs no write ([6d50ca4](https://github.com/elct9620/kobako/commit/6d50ca4d79b70956d113f0798255300aa8a7fe92))
* let a Host App bound how long a Guest Binary takes to be set up ([2addab0](https://github.com/elct9620/kobako/commit/2addab002550539e3ebb24bdd599dd424c2dc3d9))
* **runtime:** hand each invocation its Grants and let an engine declare the kinds it supports ([cba90ee](https://github.com/elct9620/kobako/commit/cba90eed09c8afcb23080cef1c9cbb3650648472)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** keep a directory descriptor's identity its own while open ([6267642](https://github.com/elct9620/kobako/commit/6267642ea831917be76a0101e8d206ccfa8834c1))
* **runtime:** let a dispatch handler abandon its invocation ([179da9e](https://github.com/elct9620/kobako/commit/179da9ee1f31fed9e6d0c149faee0ad55f8db487))
* **runtime:** let a Filesystem's file refuse its close ([af16007](https://github.com/elct9620/kobako/commit/af1600797b098796dd6f329672c7d04e2e1cd2fa)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount decide its own access as the one kind of Grant ([5c9189e](https://github.com/elct9620/kobako/commit/5c9189eea8ac61cf165dca1b38e459c4f14acdd0)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount's Policy judge the spelling the host stores ([5281057](https://github.com/elct9620/kobako/commit/52810573f22d2a3edfae9b34b7767a5f440fba15))
* **runtime:** let the records a Filesystem or an engine builds gain fields ([c0ef88d](https://github.com/elct9620/kobako/commit/c0ef88d0b6df16d651e4c49226e41667307d772f))
* **runtime:** name a file grown past what a Filesystem keeps ([7bac3e6](https://github.com/elct9620/kobako/commit/7bac3e65534097b85d93f5967b6c73a979869a78))
* **runtime:** name each code a Filesystem may fail with ([1d980fa](https://github.com/elct9620/kobako/commit/1d980fa4328bad42e8a8665adfec9fc844690e97))
* **runtime:** refuse a Mount at a path that is no absolute guest path ([a525758](https://github.com/elct9620/kobako/commit/a5257589f9d831b9a3b6d1952df6e5c8469630e1))
* **wasmtime:** answer a Filesystem out of space as ENOSPC ([c648ebf](https://github.com/elct9620/kobako/commit/c648ebf63a871b7d4417110de00decf1738a19d6))
* **wasmtime:** cap the descriptors one invocation holds open ([03409c1](https://github.com/elct9620/kobako/commit/03409c1437f71312c584a92e22054b90241535a3))
* **wasmtime:** let Config gain a cap without breaking a frontend ([55d8093](https://github.com/elct9620/kobako/commit/55d80933b958bab67f1b94deb7633272dc56f972))
* **wasmtime:** present each filesystem Grant to the guest as a preopen ([68149a7](https://github.com/elct9620/kobako/commit/68149a7adc912172b72e7a34bd39082a3b2a9e95)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Bug Fixes

* append through a Filesystem's file only in one step ([0509ca7](https://github.com/elct9620/kobako/commit/0509ca70d2d5a832303973ba0f7280be0731fd3d))
* ask a mount's block once for a read ([f89bd16](https://github.com/elct9620/kobako/commit/f89bd16c1a71ed1d385ab9d3ac5974ac3f0a9103))
* follow no link swapped in after a Policy judges a path ([ed93213](https://github.com/elct9620/kobako/commit/ed9321303dd45e113fc413e99e1240118cc4bbe4))
* hold an Overlay's lock across each call, waiting without the GVL ([b8e7342](https://github.com/elct9620/kobako/commit/b8e734273fdc577a40be97c43c46d9548d66c625))
* **io:** build Errno and IOError as CRuby does ([da77c47](https://github.com/elct9620/kobako/commit/da77c4725d3701ba982f5425b1fcf2fb0038ac89))
* **io:** give File::Constants CRuby's Linux values ([14cadd8](https://github.com/elct9620/kobako/commit/14cadd88ac5a5dcb8f73b252b68c493488b3e78a))
* **io:** keep the number a SystemCallError is built with ([5fd327a](https://github.com/elct9620/kobako/commit/5fd327add59173cd5756fecb41d97aebcba7b881))
* **io:** raise each code the engine answers as its own Errno ([eb6382e](https://github.com/elct9620/kobako/commit/eb6382e506a30e82576fa12ab9859e7d35be2b17))
* **io:** raise Errno::EIO for a file failure no other code names ([8a0447d](https://github.com/elct9620/kobako/commit/8a0447d2d09ea83e6f7771cf0411bfd6deec3d1a)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **io:** raise RangeError for a number past what putc, read and errno take ([6f87cc0](https://github.com/elct9620/kobako/commit/6f87cc08dff7146415f80ca25cd75ddbec4391c0))
* **io:** raise the error a failed file write names ([bcc8e00](https://github.com/elct9620/kobako/commit/bcc8e00662741fcac3b91f58c0b60f4a79d2ab9f))
* **io:** write a file to the end when its Filesystem takes it in parts ([029b439](https://github.com/elct9620/kobako/commit/029b43939a3a48bc2925057df69bfcb85b1a0548))
* keep a link within the directory a descriptor opened ([952dd05](https://github.com/elct9620/kobako/commit/952dd0556969683f44798da94f9fc7a17c1f6d55))
* run a stored block as the block of the Call that carried it ([bf33dc1](https://github.com/elct9620/kobako/commit/bf33dc1fd40c83498c6c12b0be52b3a835a144cf))
* **runtime:** make every Filesystem answer the methods that keep a guarantee ([00f5af6](https://github.com/elct9620/kobako/commit/00f5af63f34ac7722249746d0b0eb566da618803))
* **runtime:** report a merged directory's identity alike through an Overlay's lstat ([ded3b4a](https://github.com/elct9620/kobako/commit/ded3b4aba78efa678318c38b139b5c9c76b5d3fd))
* **wasmtime:** answer a Call the host cannot serve with the reason ([cf60dcc](https://github.com/elct9620/kobako/commit/cf60dccc63a0d0cfc959f701b7212c78beafa528))
* **wasmtime:** answer a path past Linux's limits as NAMETOOLONG ([af43917](https://github.com/elct9620/kobako/commit/af43917e30a634d6c1bd70813044967bd1057ba3))
* **wasmtime:** answer a stat that follows no link with the link itself ([ad0072e](https://github.com/elct9620/kobako/commit/ad0072e03ecc363487598a507de20a8f7aec2927))
* **wasmtime:** answer a wait to write a file by what its Mount permits ([bf863db](https://github.com/elct9620/kobako/commit/bf863db998edeab60f9276fc53e8adcf8cf98364))
* **wasmtime:** answer an offset past the largest signed offset as EINVAL ([b94788e](https://github.com/elct9620/kobako/commit/b94788e1c1fc9d86f647da4396bcba3eaec16242))
* **wasmtime:** answer an unlink with a trailing slash by the name itself ([94e7250](https://github.com/elct9620/kobako/commit/94e72505cfc027319c2139e15488eb2682268d49))
* **wasmtime:** answer descriptor flags, advice, and clock waits as preview1 does ([c1aec63](https://github.com/elct9620/kobako/commit/c1aec632445c3bf216be6375552c799d7877670f))
* **wasmtime:** answer dot paths, creates, and far seeks as Linux does ([68c234e](https://github.com/elct9620/kobako/commit/68c234e06859b93fb986bdbab92cca944ce39700))
* **wasmtime:** answer EINVAL for a guest path holding a NUL ([1c681d6](https://github.com/elct9620/kobako/commit/1c681d6e723f259ab2b3acc5e3a33d85499a2ef5))
* **wasmtime:** answer ENOENT for an open of a missing path that makes nothing ([48e3944](https://github.com/elct9620/kobako/commit/48e3944c64d7ef410fcc4595d80ba0284f257a09))
* **wasmtime:** answer every preview1 call that reads a descriptor from kobako's own table ([d0e39ae](https://github.com/elct9620/kobako/commit/d0e39aebd6d729d49111340af7f2af4e37aa7909)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **wasmtime:** answer fd_allocate beneath a Mount as unsupported ([e3d2071](https://github.com/elct9620/kobako/commit/e3d20713e4474cd637dacfece99e52b80d7ad2f6))
* **wasmtime:** answer what stands at a path before a Mount refuses an open ([81e02dc](https://github.com/elct9620/kobako/commit/81e02dc5e7d500b2971ab5d4a670d38601f89389))
* **wasmtime:** ask a Policy once for an open that may create ([a4613cb](https://github.com/elct9620/kobako/commit/a4613cb8cff89af51f55ce090433c61268105789))
* **wasmtime:** bound a buffer outside memory before a file's access is checked ([8b76f76](https://github.com/elct9620/kobako/commit/8b76f764a6336d2e77baa2ca6fe63dfd7663ce19))
* **wasmtime:** bound reads, writable waits, and a FIFO's names as preview1 hosts do ([6d89e55](https://github.com/elct9620/kobako/commit/6d89e55541f3366815dd7ed271e813a6ba41bcb3))
* **wasmtime:** bound what one preview1 call copies to wasmtime's hostcall fuel ([097c554](https://github.com/elct9620/kobako/commit/097c55426c1e71467bb1e372abc190331dd3e601))
* **wasmtime:** check path_open's flags as wasmtime-wasi does ([22eb494](https://github.com/elct9620/kobako/commit/22eb4944117260831e92fe616b02888078c02b0c))
* **wasmtime:** close the file fd_renumber moves a descriptor onto ([0f6148d](https://github.com/elct9620/kobako/commit/0f6148d536ed3e9e96749c2e4a60bf6275332655))
* **wasmtime:** give each writer of a cached artifact its own temp file ([995a46e](https://github.com/elct9620/kobako/commit/995a46e99dc1c485a303bb7f023b4e668a2801a3))
* **wasmtime:** grant the compile instance no ambient authority ([c3edc71](https://github.com/elct9620/kobako/commit/c3edc71ab99dac57cbabfc685a7cc60a5a5c06ac))
* **wasmtime:** hold the whole invocation to its deadline ([231e325](https://github.com/elct9620/kobako/commit/231e32592699008adf3105050bee8b5528ea27a8))
* **wasmtime:** judge the cache directory itself, not where a link leads ([6baf6fd](https://github.com/elct9620/kobako/commit/6baf6fd2ad845cea83049c4b2bb51174aa205cd0))
* **wasmtime:** keep a write and its file position within the largest signed offset ([29d4c34](https://github.com/elct9620/kobako/commit/29d4c341fc5a47ee43fdd249918e1f3e68120f86))
* **wasmtime:** key the compiled-artifact cache by the engine that compiles it ([b43bad2](https://github.com/elct9620/kobako/commit/b43bad2fe176c153ed82a3a8825698414df78dfa))
* **wasmtime:** leave a link named with a trailing slash in place ([9b3cdea](https://github.com/elct9620/kobako/commit/9b3cdea6dc00353c5c4896ec29b88eebdaddc8ab))
* **wasmtime:** list a directory once for each listing a guest reads ([c333001](https://github.com/elct9620/kobako/commit/c3330011725c3dba01ea248ed7d1dfea31085af0))
* **wasmtime:** look up what stands before a dot path as Linux does ([0898eaf](https://github.com/elct9620/kobako/commit/0898eafb55397c84ca9007561ddb35dd007ed2b8))
* **wasmtime:** place an append at the end as the file stands when it lands ([d7291c4](https://github.com/elct9620/kobako/commit/d7291c4f32fb1f5f2494c69c76796476bbcf4d43))
* **wasmtime:** reach the artifact cache through the directory it checked ([462fe91](https://github.com/elct9620/kobako/commit/462fe918d516cd639e405b06a5e280a9e58624df))
* **wasmtime:** refuse a `..` that steps out of a host link ([4599386](https://github.com/elct9620/kobako/commit/45993868c7334376f5d1a455afbd15fe4a2146bc))
* **wasmtime:** refuse a directory made in an opened directory's place ([5e67d4c](https://github.com/elct9620/kobako/commit/5e67d4cd2a52e3aa37bdd615fe1ae101a618db88))
* **wasmtime:** refuse a link an open asks not to follow ([123a01b](https://github.com/elct9620/kobako/commit/123a01b01f59cc69c0e3a23f9aeb01aa89132c07))
* **wasmtime:** refuse a link beneath a Policy before the Policy judges the path ([0e08e7b](https://github.com/elct9620/kobako/commit/0e08e7b463344ae34b5feb1787f8af8a6c719063))
* **wasmtime:** refuse a link to a directory an open asks not to follow ([eec218f](https://github.com/elct9620/kobako/commit/eec218fdd5584f641070032cf1110ef1683eaae5))
* **wasmtime:** refuse a path climbing above the directory it is opened through ([ae52746](https://github.com/elct9620/kobako/commit/ae52746761624249b7ff1e1fa09ece4ea8130ced))
* **wasmtime:** refuse a path through a link beneath a Mount with a Policy ([3da1641](https://github.com/elct9620/kobako/commit/3da16416692ade01db2d9cf366d54d3ec5f50605))
* **wasmtime:** refuse a rename of a file onto a path ending in a slash ([486a0ab](https://github.com/elct9620/kobako/commit/486a0ab3584772b58e181d5de57ef505e610eb78))
* **wasmtime:** refuse an argument preview1 cannot hold in wasmtime-wasi's order ([9c76e21](https://github.com/elct9620/kobako/commit/9c76e2181a0dabd81f02723154cc4290215fcf70))
* **wasmtime:** refuse to remove or move a Mount's root ([9f2e000](https://github.com/elct9620/kobako/commit/9f2e000115a4dd4aecfcc4b1cd39b23f6f20fe05))
* **wasmtime:** report a host FIFO or device as of an unknown filetype ([5a212e7](https://github.com/elct9620/kobako/commit/5a212e7a7c0f069ffdcc56b850fab3778863d9ee))
* **wasmtime:** require wasmtime 49.0.4 for the aarch64 br_table fix ([390e30b](https://github.com/elct9620/kobako/commit/390e30ba51ed4175820f2bb230e3ad5b0ad1e9a5))
* **wasmtime:** resolve a `..` and a trailing slash as a host does ([d2d04c1](https://github.com/elct9620/kobako/commit/d2d04c1fff24d8829b2cf4fcbf628565f35eeadc))
* **wasmtime:** resolve a final dot through the name before it ([7a0d3d8](https://github.com/elct9620/kobako/commit/7a0d3d8b23d47d4ba5c942682841434e74d6bf6a))
* **wasmtime:** resolve a link a stat names with a trailing slash ([6da98fa](https://github.com/elct9620/kobako/commit/6da98facbccaa9bf6d8f8280a56841e19a7c879b))
* **wasmtime:** size a file opened only to read as wasmtime-wasi and Linux answer ([810f5c8](https://github.com/elct9620/kobako/commit/810f5c8e863dc5bdbe82f8a19c7559fd7eee03f9))
* **wasmtime:** sync a cached artifact before renaming it into place ([097efc7](https://github.com/elct9620/kobako/commit/097efc77599545bb2a5e729c75d23082470ed8f4))
* **wasmtime:** take wasmtime 49.0.3 for its sandbox advisories ([7672592](https://github.com/elct9620/kobako/commit/76725923cac50473c7e3a21cc5121e3cbba0cba5))
* **wasmtime:** trap a guest field address past the address space ([de5d47e](https://github.com/elct9620/kobako/commit/de5d47e10aa5d978573e46a7cb3453ff248d006e))


### Performance Improvements

* **io:** answer IO#read with a length in one read the file holds ([e9161a2](https://github.com/elct9620/kobako/commit/e9161a2018c1617157025de275f77890e865b7d1)), closes [#78](https://github.com/elct9620/kobako/issues/78)

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.17.0...kobako-wasmtime-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* the kobako and kobako-wasmtime crates declare a minimum supported Rust version of 1.96.
* **transport:** the stdin layout a Guest Binary reads changed within ABI 4. kobako_transport::envelope::Frames is replaced by Preamble, which is #[non_exhaustive] and built from Default; Bindings and Snippets no longer have their own decode and encode. Guest::eval and Guest::run, MrbGuest::eval and MrbGuest::run, and Runtime::invoke take a Preamble in place of Frames. kobako_core re-exports Preamble and Flag in place of Frames.
* **transport:** the ABI version is 4, so a Guest Binary built for ABI 3 is refused at Sandbox construction. kobako_core::export_guest! emits __kobako_downcall in place of __kobako_eval, __kobako_run, __kobako_yield_to_block and __kobako_take_outcome, and the host import is __kobako_upcall. kobako_core::abi::{take_outcome, write_outcome, write_reply, decode_run} are replaced by serve and answer, and the frames module is no longer public. Reply, YieldReply and Outcome gain an Unsupported arm and take their tags from the shared registry, and kobako_core::DispatchError and kobako::YieldError gain an Unsupported arm. A yield to a guest without block support raises Kobako::UnsupportedError at the Service's yield instead of failing the invocation as a trap.
* **runtime:** Runtime::invoke takes &kobako_transport::envelope::Frames in place of kobako_runtime::Frames, whose byte fields are gone. Guest::eval and Guest::run (and their MrbGuest flows) receive the decoded Frames, eval also its source; kobako_core::frames::read_frame is replaced by read_eval_frames and read_frames.
* **runtime:** kobako_runtime::snapshot::Completion::Outcome holds a decoded kobako_transport::envelope::Outcome instead of Vec<u8>; an engine decodes the result itself and reports an unframed one as
* **runtime:** kobako_runtime::runtime::Entry::Run holds { entrypoint: &str, payload: Vec<u8> } instead of { envelope: &[u8] }; a frontend passes the fields, and an engine encodes the Run envelope itself.
* **runtime:** kobako_runtime::yielder::Yielder::yield_to_block returns Result<YieldReply, Trap> instead of the encoded bytes; an engine implementing it decodes the reply itself.
* a memory budget or deadline reached during a Service answer or a yielded block now raises Kobako::MemoryLimitError or

### Features

* **guest:** raise Kobako::UnsupportedError when a host does not serve a call ([db003af](https://github.com/elct9620/kobako/commit/db003af75cf2d11b5f644986aa77c0408f5f59ba)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** compile a Preamble's snippets on an instance of their own ([303432d](https://github.com/elct9620/kobako/commit/303432dd90ed62dc378391515e68e784e1b473a4)), closes [#75](https://github.com/elct9620/kobako/issues/75)
* **transport:** carry each operation as a tag on one channel per direction ([c7d574b](https://github.com/elct9620/kobako/commit/c7d574b59a42e58632f2bdc46cc9a8e4c7ac6121)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** keep an unknown request's body, and refuse tag 0x00 ([edb64df](https://github.com/elct9620/kobako/commit/edb64df8f87694d17c5b6fd71ac63329023306ec)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** set an invocation up from a Preamble of tagged records ([84d4498](https://github.com/elct9620/kobako/commit/84d4498464f94d7312f26931ddd4ec43894a6654))


### Bug Fixes

* end the invocation as the trap a host callback into the guest hits ([b8f47d3](https://github.com/elct9620/kobako/commit/b8f47d3699a19adcfd5596af93136401a286c761))
* keep a compile's bytecode only when the Preamble still fits it ([765db08](https://github.com/elct9620/kobako/commit/765db08ad31c8f5163794c7e144355f3237af93b))
* **wasmtime:** keep a compile answer only when it covers every source snippet ([93e9ddd](https://github.com/elct9620/kobako/commit/93e9ddd79f714446ae713a3145c59d3d2f4145dc))
* **wasmtime:** tell every frontend how to recover, not only the gem ([cab805a](https://github.com/elct9620/kobako/commit/cab805ab62441a0ca2c78775b7a0d7ce9cc05c43))
* **wasmtime:** word a missing or mistyped export one way ([8ab0aee](https://github.com/elct9620/kobako/commit/8ab0aeec8bda2960b6cab0502ef51860ea9093b8))


### Code Refactoring

* **runtime:** hand a frontend the decoded Outcome ([0da6aac](https://github.com/elct9620/kobako/commit/0da6aac0f7943d694a5a4ab9439b2d6c527ae1b0)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** hand a frontend the decoded Yield Reply ([e80ab54](https://github.com/elct9620/kobako/commit/e80ab54e36c2a80b0f746c675f85bd1238fd4331)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the ABI endpoints frame and read the Frames ([dea069a](https://github.com/elct9620/kobako/commit/dea069ac49a59c12a6f7102ed3cf2925f316cbff)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the engine frame a run's entrypoint and payload ([62f9c94](https://github.com/elct9620/kobako/commit/62f9c9407ff3ce690723625bea075fa49d2dda47)), closes [#73](https://github.com/elct9620/kobako/issues/73)


### Build System

* declare the Rust 1.96 floor wasmtime 49 sets ([f49a2df](https://github.com/elct9620/kobako/commit/f49a2df88fa51d3b0eecb619cab1e98d79bb4245))

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.16.0...kobako-wasmtime-v0.17.0) (2026-09-20)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.15.0...kobako-wasmtime-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.14.0...kobako-wasmtime-v0.15.0) (2026-09-13)


### Features

* **spec:** bring the Rust frontend under the behaviors it answers ([b62243f](https://github.com/elct9620/kobako/commit/b62243f68b338fd219136d60c9cf760d0be03fc0))
* **spec:** declare the bytes both sides must already agree on ([3875755](https://github.com/elct9620/kobako/commit/3875755999550511837808dcd24a25086bedc926))
* **spec:** declare what the driver holds a run to ([4539cc1](https://github.com/elct9620/kobako/commit/4539cc16a7ddb6c1706c36282e64182bb480b684))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.13.1...kobako-wasmtime-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.13.0...kobako-wasmtime-v0.13.1) (2026-07-30)


### Bug Fixes

* **host:** keep nothing in the ABI probe's capture pipes ([a3d6c1c](https://github.com/elct9620/kobako/commit/a3d6c1c8edb9964a7d8ab162b2e379b39dbbddb9))


### Performance Improvements

* **host:** ask an artifact its ABI version once, not once per Sandbox ([693b94f](https://github.com/elct9620/kobako/commit/693b94f878e56815fe3faeae94e5c4d0cd9c8bb6))

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.12.0...kobako-wasmtime-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **wasmtime:** put every cap in one struct, named the way the field is read
* **runtime:** give the engine contract names an implementer can write
* **transport:** name the core envelope's types after what they carry
* **wire:** route every tier through the one envelope
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))


### Code Refactoring

* **runtime:** give the engine contract names an implementer can write ([0a41491](https://github.com/elct9620/kobako/commit/0a41491360cf17def8245c0758679e92572b990f))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **wasmtime:** put every cap in one struct, named the way the field is read ([3f57860](https://github.com/elct9620/kobako/commit/3f578608b044d1ade0ae35231266cf3c9e517c02))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.11.0...kobako-wasmtime-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.2...kobako-wasmtime-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.1...kobako-wasmtime-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.0...kobako-wasmtime-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.9.0...kobako-wasmtime-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.8.0...kobako-wasmtime-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.7.0...kobako-wasmtime-v0.8.0) (2026-07-08)


### Bug Fixes

* **crates:** keep the no-timeout epoch deadline within range ([a3255df](https://github.com/elct9620/kobako/commit/a3255df98a77825bb39b571a60fe6ff83d269d19))

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.6.1...kobako-wasmtime-v0.7.0) (2026-07-03)


### Features

* **crates:** build the requested isolation profile into the WASI context ([63c25d8](https://github.com/elct9620/kobako/commit/63c25d835d4d03010c1658217cee412318e6b5d8))
* **runtime:** runtimes declare their isolation profile ([f89717a](https://github.com/elct9620/kobako/commit/f89717a6bc9809f0de0df78f97da33a49a1474ac))

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.6.0...kobako-wasmtime-v0.6.1) (2026-07-02)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

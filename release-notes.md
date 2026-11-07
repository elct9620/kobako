:robot: I have created a release *beep* *boop*
---


<details><summary>0.30.0</summary>

## [0.30.0](https://github.com/elct9620/kobako/compare/v0.29.0...v0.30.0) (2026-11-07)


###   BREAKING CHANGES

* **regexp:** refuse a braced hex escape and the absence operator
* **regexp:** read \w, \d, \s, \b and POSIX brackets by the (?a), (?d) and (?u) options
* **ext:** a mount's block is asked :read_write before an open that may create finds what stands, so an open of an existing file asking to create is judged as :read_write and then as the access it opens for, where it was judged once.
* **runtime:** Kstat's len, accessed, modified, and changed fields are now size, atime, mtime, and ctime, and Profile::guest_metadata is now
* **runtime:** spell each file type as Linux's FT_* names it
* **runtime:** name what a path reports Kstat, as Linux's struct kstat
* **runtime:** build an open's flags once and hand them to do_file_open
* **runtime:** answer EAGAIN to a `..` a move landed before
* **runtime:** judge a path through a link where the link leads
* **runtime:** kobako_runtime::Denial is gone. A Denial::ReadOnly is now
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
* **io:** list, make, and remove directories through Dir ([1b862a4](https://github.com/elct9620/kobako/commit/1b862a4b9d0d7d16a287585bef8d2615d4457242))
* **io:** sync, truncate, and read or write a file at an offset ([7f61b7b](https://github.com/elct9620/kobako/commit/7f61b7b26fef46e1cb5996e04a5c1d1c6ae81536))
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
* **runtime:** ask a Filesystem's permission where Linux's VFS does ([2786574](https://github.com/elct9620/kobako/commit/2786574efe2401f98fa8650848ee09c4be0335aa))
* **runtime:** bound how many Layers an Overlay stacks, as Linux does ([1c88f06](https://github.com/elct9620/kobako/commit/1c88f063d7664c9a020083df863b33b66df737c8))
* **runtime:** follow an Overlay directory through its moves ([858d5d7](https://github.com/elct9620/kobako/commit/858d5d75dbb2246475a0f3938b527a0555cc3ab9))
* **runtime:** judge a path through a link where the link leads ([7407a71](https://github.com/elct9620/kobako/commit/7407a71cfbcb9c4f5faa3ef954c592d0687174cc))
* **runtime:** keep the times a caller sets on a Tmpfs, a Bind, and an Overlay ([d53d547](https://github.com/elct9620/kobako/commit/d53d5479c48e4d423addf02b7d373784409d1604))
* **runtime:** let a file answer its own seek ([fee742a](https://github.com/elct9620/kobako/commit/fee742addc2cbcd04e4cca387b399b419a2eab5d))
* **runtime:** let a Filesystem hand one inode for many names ([fc34fc6](https://github.com/elct9620/kobako/commit/fc34fc6ff7dfb33daa492cb5df390b9615ca4d04))
* **runtime:** let a Filesystem make a file through create ([58ab067](https://github.com/elct9620/kobako/commit/58ab06766daa6d173e730941efcd11acc21ed9d5))
* **runtime:** lock an Overlay as Linux locks overlayfs, so its calls run at once ([b547dcd](https://github.com/elct9620/kobako/commit/b547dcd1abb3c33b74b0359692443e812fcb06ec))
* **runtime:** lock each directory a call changes, as Linux does ([126694b](https://github.com/elct9620/kobako/commit/126694b826c430a4b62079ddc4872873e743ef12))
* **runtime:** mark and copy up an Overlay's changes as overlayfs does ([4ea3f5f](https://github.com/elct9620/kobako/commit/4ea3f5f319d79314e1e4a80df32e6ff094bad377))
* stack an Overlay as overlayfs does, under an upper Tmpfs ([2b59780](https://github.com/elct9620/kobako/commit/2b597806809d7a810d29265e9e377d7d648d77f9))
* walk a guest path one name at a time and follow links there ([2e57fdc](https://github.com/elct9620/kobako/commit/2e57fdc11c5cbeb00697e9b287f59c15f0c91e2d))


### Bug Fixes

* ask a Ruby Filesystem by the path its directory stands at now ([c5471c0](https://github.com/elct9620/kobako/commit/c5471c0989ea085751f5c888c4c79b8ac9958cd0))
* **ext:** leave an open making nothing to the runtime in a Ruby Filesystem ([4e05b5f](https://github.com/elct9620/kobako/commit/4e05b5fb64051ea545752c02eef193f572230b40))
* **ext:** let a Thread's interrupt stop a Sandbox's waits for a lock ([894ff5b](https://github.com/elct9620/kobako/commit/894ff5b1a5e95a95023b47ed7d53134e67e98baf))
* keep an opened directory's identity through the directory itself ([28ee9e0](https://github.com/elct9620/kobako/commit/28ee9e003a30fb9159ff3f6c47dce2da25586eb2))
* open a Ruby Filesystem's file on a host whose Ruby has no NOFOLLOW ([99943cf](https://github.com/elct9620/kobako/commit/99943cf0e8abd08ff92e5f38645ea6ab111c8510))
* **regexp:** answer a condition that names a group ([faedca6](https://github.com/elct9620/kobako/commit/faedca617db4c859cd8bc9fcab3a599c6a188b22))
* **regexp:** carry capture names into a substitution block's match ([e3230b7](https://github.com/elct9620/kobako/commit/e3230b7213c2a70e14396408526a85ffa487621b))
* **regexp:** fold a quantifier on a group holding one quantified unit ([09ff373](https://github.com/elct9620/kobako/commit/09ff37345d31d81fb27fc59c5db5213aea0bc00a))
* **regexp:** fold case-insensitive classes as CRuby folds them ([ad1b211](https://github.com/elct9620/kobako/commit/ad1b211afd8bad77247bd9428d0242abe6cbae03))
* **regexp:** fold stacked quantifiers and repeat a look-around as CRuby does ([0ea692d](https://github.com/elct9620/kobako/commit/0ea692d1dce5622b90dc54d55c4a64efb90ff38d))
* **regexp:** leave the match globals as MRI does after finding every match ([3d3a7f9](https://github.com/elct9620/kobako/commit/3d3a7f9673f15fd63cb810dc4bf564388fa8e24b))
* **regexp:** match case-insensitive text across a multi-character fold ([0cb10e1](https://github.com/elct9620/kobako/commit/0cb10e1a363ab51a31198af9363e9493d77991e2))
* **regexp:** read \p{Graph}, \p{Print} and the properties the engine lacks as CRuby does ([8784d73](https://github.com/elct9620/kobako/commit/8784d73184f090cc915a97d4dcef65ef0d98efe5))
* **regexp:** read \w, \d, \s, \b and POSIX brackets by the (?a), (?d) and (?u) options ([655b5e5](https://github.com/elct9620/kobako/commit/655b5e5a6125726b91ceaaf540394d429a38709d))
* **regexp:** read a backreference level when the pattern does not recurse ([7ac2b7b](https://github.com/elct9620/kobako/commit/7ac2b7b648576e5160141436b905bac29945479f))
* **regexp:** read a shared capture name as MRI does ([e855a5c](https://github.com/elct9620/kobako/commit/e855a5cc6588773e5852bc1b8d7230b569352dec))
* **regexp:** read inline options as MRI does ([191515b](https://github.com/elct9620/kobako/commit/191515b9cb49170156e4c78fac66c710a16e315b))
* **regexp:** read intervals as MRI does ([6eb853e](https://github.com/elct9620/kobako/commit/6eb853e6bfd69fd4a30f6f43f0df9c4f45965756))
* **regexp:** read octal, control and meaningless letter escapes as CRuby does ([e7b4355](https://github.com/elct9620/kobako/commit/e7b4355ad0bd597c7dce85ea7a5bd5270a1663e8))
* **regexp:** refuse a braced hex escape and the absence operator ([0c29dd8](https://github.com/elct9620/kobako/commit/0c29dd8358845f733a0de9ef5d816749655e857e))
* **regexp:** refuse a word boundary without the Unicode build ([d666e1f](https://github.com/elct9620/kobako/commit/d666e1fd54d890ffea61d9e78ae6723f7ceacd07))
* **regexp:** resolve a backreference by a shared name as MRI does ([90a7100](https://github.com/elct9620/kobako/commit/90a7100fcba1122c5f132658df68a709ecec1755))
* report the size a Ruby Filesystem's stat answers for a directory ([19211cf](https://github.com/elct9620/kobako/commit/19211cf4b2d621928be8bb098f172bec89cc759a))
* **ruby:** keep an interrupt from jumping over a run's native frames ([7d12d6a](https://github.com/elct9620/kobako/commit/7d12d6adae69d2e293226684aa827dac198b6852))
* **ruby:** list what a Ruby Filesystem still holds and show its links as links ([b1097e7](https://github.com/elct9620/kobako/commit/b1097e7ebec3905224135775f96c0cf8c134daa8))
* **runtime:** answer a dot path or a slash create through a link as Linux ([8aafe42](https://github.com/elct9620/kobako/commit/8aafe42cd517cdc9730ac41b092000570c065511))
* **runtime:** answer a link call by what stands, as filename_linkat does ([5a0ac25](https://github.com/elct9620/kobako/commit/5a0ac257a260f8638caf794df50d95ad11fb139d))
* **runtime:** answer each path call in Linux's order ([85d0407](https://github.com/elct9620/kobako/commit/85d040761699ca72d3a1e2ec5737d9f9ccfcbd47))
* **runtime:** answer EAGAIN to a `..` a move landed before ([1aa66dd](https://github.com/elct9620/kobako/commit/1aa66dda78bd63aff91a78bfe8d6bff6306a9c19))
* **runtime:** answer ENODEV to an allocation on a device ([0ba099b](https://github.com/elct9620/kobako/commit/0ba099b474890179e3c4c4e8a13837b9be5b5ead))
* **runtime:** ask a walk's start whether it still stands under its directory's lock ([4b14e4e](https://github.com/elct9620/kobako/commit/4b14e4e91aaf1a91d2815789ed41c2da9c50e97d))
* **runtime:** ask an entry about itself however a guest reached it, as fstat and ftruncate do ([1166426](https://github.com/elct9620/kobako/commit/1166426d3f4bc27d9c2d0d3a8f2b7f1a6a23f59e))
* **runtime:** ask atomic_open for an open's last name no walk looked up ([9860ae9](https://github.com/elct9620/kobako/commit/9860ae9391874cf27bf63345b4672458f696a8a8))
* **runtime:** ask only the dentries an Overlay's entry keeps, and name each helper for what it does ([bb5613c](https://github.com/elct9620/kobako/commit/bb5613c33d1d4e20640fba3c4a1a605fd82cade1))
* **runtime:** ask the readlink hook only of a link ([d584cbb](https://github.com/elct9620/kobako/commit/d584cbb66d34672f49cb44a0226071277a029275))
* **runtime:** bound the host directories a Bind keeps open ([531cf3a](https://github.com/elct9620/kobako/commit/531cf3a6f84bc706d00a9eb91bdafb4f76962f6e))
* **runtime:** build an open's flags once and hand them to do_file_open ([6672734](https://github.com/elct9620/kobako/commit/6672734fc42af8c2040402c8ed2b14d562c31328))
* **runtime:** climb an Overlay's directories in a loop ([70706f3](https://github.com/elct9620/kobako/commit/70706f34d121e335d64cc05fe255cd67f67af614))
* **runtime:** copy a file up before setting its times through an Overlay ([1a20b59](https://github.com/elct9620/kobako/commit/1a20b59c0db975ebe2bc36f7d5a051b64f325940))
* **runtime:** count a held listing by what it holds on the heap ([8425dab](https://github.com/elct9620/kobako/commit/8425dabd1cdf19450d3fff0510850972e3868633))
* **runtime:** count no name for a Tmpfs directory that was removed ([bccbd99](https://github.com/elct9620/kobako/commit/bccbd992f2de8e23496a2cf81a33e4653d18ce31))
* **runtime:** empty a file an open truncates through setattr ([0ee006f](https://github.com/elct9620/kobako/commit/0ee006f4683ed3059a9f3251d450bf3de0eb365d))
* **runtime:** find and create an O_CREAT open's file under one directory lock ([a3684d4](https://github.com/elct9620/kobako/commit/a3684d4ca79e7b8ae518118adf9ec965ba46ba9a))
* **runtime:** hand a listed entry where it stands, as dir_emit does ([804f8b2](https://github.com/elct9620/kobako/commit/804f8b2ce93a8d5b935d400d295ba5f2ce7c5a32))
* **runtime:** hide a name as an Overlay's upper holds it when the removal lands ([14a84c6](https://github.com/elct9620/kobako/commit/14a84c62fde74c7347cf16bc2002104022e64ad0))
* **runtime:** hold a Bind's directory descriptor to what its dentry recorded ([f9e7fc9](https://github.com/elct9620/kobako/commit/f9e7fc99ab37a7f41eb67661bbf7c8c1466ff090))
* **runtime:** hold an Overlay directory's Layers instead of reaching them again ([85d5539](https://github.com/elct9620/kobako/commit/85d5539ee0ad6c98e0b20e376f540fcaf704ac16))
* **runtime:** hold what a guest's listings keep within its memory limit ([e4d74a5](https://github.com/elct9620/kobako/commit/e4d74a5ad2d5554e1b509ee606c1911811f66be8))
* **runtime:** judge a time set through a descriptor where it stands ([e1e5147](https://github.com/elct9620/kobako/commit/e1e51475c63ea20593b28b86fdc59fce7e51e019))
* **runtime:** keep a Bind's walk and allocation answers true under a race ([cce1a2a](https://github.com/elct9620/kobako/commit/cce1a2a0e64bf44648e01caf14e901f1cb04fad3))
* **runtime:** keep a name nothing stands at, as each Filesystem decides ([8ecee7e](https://github.com/elct9620/kobako/commit/8ecee7ebbed65fa3c7415e98043d3b130af5971a))
* **runtime:** keep an Overlay's copy up to what it made itself ([d16f9e4](https://github.com/elct9620/kobako/commit/d16f9e4efae77289701d5976c947f7626674bf73))
* **runtime:** keep pipes among the Filesystems, reading nothing into no room ([79a2240](https://github.com/elct9620/kobako/commit/79a22401f9c8ab3c8fe777955a4b39e880418686))
* **runtime:** keep the flags a directory was opened with ([e328aa4](https://github.com/elct9620/kobako/commit/e328aa4d6ef59aaef970d3b9e162f5edeb9d6222))
* **runtime:** keep what preview1's rights mean out of the VFS open ([fd78dce](https://github.com/elct9620/kobako/commit/fd78dce168149cb439f65ab0db74f0b655b9e35f))
* **runtime:** key the inode number a permissive guest reads ([1d2b8b2](https://github.com/elct9620/kobako/commit/1d2b8b276c805aba557b336bae6cca31fd330570))
* **runtime:** leave a move between two names of one entry alone ([717d321](https://github.com/elct9620/kobako/commit/717d3214ffc7a0a11527898a1b0c067f6d53b42d))
* **runtime:** leave the descriptor table to the Driver, as FD_ADD reserves first ([12e6b65](https://github.com/elct9620/kobako/commit/12e6b65b4b47bdd4e73465f904c9cef92c27e1bd))
* **runtime:** let a descriptor's stat and listing ask no hook ([48d265a](https://github.com/elct9620/kobako/commit/48d265ab83773e1ee436f06d4f822bf57f372ba2))
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
* **runtime:** let only build_open_flags make an open's flags ([d1ba0ca](https://github.com/elct9620/kobako/commit/d1ba0ca306d59b533abced2eb8927de12afbb85d))
* **runtime:** list a directory as its Filesystem says, through dir_emit ([c7447a1](https://github.com/elct9620/kobako/commit/c7447a137a5237b2e071ba6500fddd736abe5092))
* **runtime:** merge only directory over directory in an Overlay ([6473956](https://github.com/elct9620/kobako/commit/6473956fd773a858ea34880b6a1d7b8e60018f3f))
* **runtime:** move seeking, flags, hints, allocation and readiness into File ([5d1cd6b](https://github.com/elct9620/kobako/commit/5d1cd6b1d1d626945b5f5c70a5342a8576a017f8))
* **runtime:** open a file made by create alone after the security modules ([5f8d30c](https://github.com/elct9620/kobako/commit/5f8d30c48e08450fa3b1065954fad8ea8e8e5a26))
* **runtime:** open a made file once its directory is unlocked ([1a61088](https://github.com/elct9620/kobako/commit/1a61088b4adbbeaef052e83ad6d8ac19b79d7523))
* **runtime:** open each Mount's root before its Policy applies ([4580697](https://github.com/elct9620/kobako/commit/45806973ab0b99f977b7d90253f8dafa9b1f8e8b))
* **runtime:** open only the entry a call judged on a Filesystem reached by path ([6a650e5](https://github.com/elct9620/kobako/commit/6a650e5f92a4ca06234e613b5dac8f3714de5e89))
* **runtime:** read `..` from where a directory stands, and end a walk beneath its start ([f43e34d](https://github.com/elct9620/kobako/commit/f43e34d7f4d80a0b24ced69e50058995148760a0))
* **runtime:** read a call's flags and path before where it starts ([70bce0f](https://github.com/elct9620/kobako/commit/70bce0f0d5b282e1324ab042d5c29b2e523cb0ae))
* **runtime:** read a dentry's parent and name together when asking if it was removed ([4548d8e](https://github.com/elct9620/kobako/commit/4548d8eac4ba5af76e0c5d29d092bf1cea29be0c))
* **runtime:** refuse a change at an Overlay's super block when it has no upper ([7f4c31e](https://github.com/elct9620/kobako/commit/7f4c31e8db91d09fc6a133708e14f28a26d60675))
* **runtime:** refuse a name no Layer holds before an Overlay changes anything ([f7ed6da](https://github.com/elct9620/kobako/commit/f7ed6da700c2f8b8c6f91b43421b7721ecd648e4))
* **runtime:** refuse one instance as two Layers of an Overlay ([b2c2206](https://github.com/elct9620/kobako/commit/b2c22060f9eceefb2e6a6048bde3960aa4d11dc9))
* **runtime:** refuse removing a path ending in .. as ENOTEMPTY, as Linux does ([a02bac9](https://github.com/elct9620/kobako/commit/a02bac9e9ea300013e90ab396226fe03fbf45f1d))
* **runtime:** refuse to open a directory the host lets no one read ([8dcf10b](https://github.com/elct9620/kobako/commit/8dcf10ba91fdbf196d80ab9dc587eb381091b283))
* **runtime:** refuse to open a link before its Filesystem is asked ([54b0349](https://github.com/elct9620/kobako/commit/54b03493790711056dd051e4492800e444a7d997))
* **runtime:** remove and move through an Overlay's upper as Linux's VFS helpers do ([7973488](https://github.com/elct9620/kobako/commit/79734884adc44c840376b10a0f3dcbec146f6830))
* **runtime:** report the kind a host gives a special file ([b421805](https://github.com/elct9620/kobako/commit/b421805dc4b76cc76f541d1b9e0579329e72a502))
* **runtime:** reserve the room a guest allocates in a Tmpfs and a Bind ([dd94317](https://github.com/elct9620/kobako/commit/dd94317257c3bbbbfd2a2a170a9d5a3783801f43))
* **runtime:** stop asking where a call starts once moves meet every pass ([6990a1c](https://github.com/elct9620/kobako/commit/6990a1c7a33dd9e9e30a7c83398ebf8ed89e8f5b))
* **runtime:** sync a Bind's root and a Ruby Filesystem's directory ([08be1b5](https://github.com/elct9620/kobako/commit/08be1b5975b50e20ca3872649775f92bf617794f))
* **runtime:** sync only as far as asked, cap one call, and let a writer through ([5e2902f](https://github.com/elct9620/kobako/commit/5e2902f95312c985f78053cacf6374db893e174f))
* **runtime:** take O_SYNC and O_DIRECTORY's other flags as Linux does ([ba6bc57](https://github.com/elct9620/kobako/commit/ba6bc57e1ad8264e9035d9808098efe5b846f32f))
* **runtime:** walk out of a link and create through one as Linux does ([c6e4054](https://github.com/elct9620/kobako/commit/c6e40547e52421bb7f8a1f86e549c40f365002ad))
* **wasmtime:** let a directory renamed onto its own path stay ([5fa99ab](https://github.com/elct9620/kobako/commit/5fa99ab7052cf4dbbd6be34bc2d79351d42e6311))
* **wasmtime:** refuse a link to an absolute path through preview1 ([3477e84](https://github.com/elct9620/kobako/commit/3477e840cd15ac4bd6f06105e4ee9842717a05bc))


### Performance Improvements

* **runtime:** ask an Overlay's Layers about each name once ([2528559](https://github.com/elct9620/kobako/commit/25285596d8f2a64bca18f16d6a5f7bfaea6e4d0d))
* **runtime:** ask an Overlay's lowest Layer for no markers ([e101335](https://github.com/elct9620/kobako/commit/e101335e53be004359c2851d7723a2bdd9155d5d))
* **runtime:** ask each Layer once for a name an Overlay looks up ([a17f601](https://github.com/elct9620/kobako/commit/a17f601300f52f7d8d6d4d928c9e3fcf682879e0))
* **runtime:** keep names found standing for the next walk ([51dfee4](https://github.com/elct9620/kobako/commit/51dfee488898f130cb0efdb7cb59d1b7a0a38957))
* **runtime:** open the directories a path walks through in one call ([453ad84](https://github.com/elct9620/kobako/commit/453ad84fed5f84b9f16fcf6148482e90c3280844))
* **runtime:** skip the whiteout lookups an Overlay already answered ([69aaefc](https://github.com/elct9620/kobako/commit/69aaefcc74bcf039a65abcbc6c6e113d29098eb1))
* **runtime:** walk a directory one Overlay Layer shows as that Layer does ([e93dd53](https://github.com/elct9620/kobako/commit/e93dd53a42404d304a94da9b6ea89a5b5f06f009))


### Code Refactoring

* **runtime:** answer a close through flush, as close(2) reads f_op-&gt;flush ([c34eb58](https://github.com/elct9620/kobako/commit/c34eb58b297e5be6ef249af03c6d7655cfd94729))
* **runtime:** answer every VFS call with an errno ([84a0a38](https://github.com/elct9620/kobako/commit/84a0a385149f5a9194fcab9598849cf7c92c46ae))
* **runtime:** hang each entry's operations on its inode, as Linux does ([e21470b](https://github.com/elct9620/kobako/commit/e21470b625255eb85b4b5ff868872f2db5a7967e))
* **runtime:** land each read and write through a Kiocb, as read_iter and write_iter ([bc35dfd](https://github.com/elct9620/kobako/commit/bc35dfd13085b6aee517748996647bd7083cdbc8))
* **runtime:** let the mount layer own what a Guest may do beneath a Mount ([0dcde21](https://github.com/elct9620/kobako/commit/0dcde2143d7935a8e4b205bb03d664974af7edfb))
* **runtime:** move how a thread waits for a lock beneath the VFS ([616f1c8](https://github.com/elct9620/kobako/commit/616f1c82bd946c3501727e6e5e17d842f5b20ae7))
* **runtime:** name a Filesystem's make-and-open atomic_open ([57a6e10](https://github.com/elct9620/kobako/commit/57a6e10cb14e7a7f6ff02d2a7a88b5e18e459165))
* **runtime:** name what a path reports Kstat, as Linux's struct kstat ([0f43bed](https://github.com/elct9620/kobako/commit/0f43bed4242c2236e02c88d00470d1d31180219e))
* **runtime:** name what an open file answers as Linux's file_operations ([ca835e9](https://github.com/elct9620/kobako/commit/ca835e99b821845bcfa56648ff7336c67242bbd7))
* **runtime:** spell a Kstat's fields as Linux's struct kstat does ([ec73cac](https://github.com/elct9620/kobako/commit/ec73cac38e9b7e8b9162ea59ebc35de7e9394c8d))
* **runtime:** spell each file type as Linux's FT_* names it ([796fac4](https://github.com/elct9620/kobako/commit/796fac412cae9c70b6f80e7f33946fe960ed520e))
</details>

<details><summary>kobako-core: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.19.0...kobako-core-v0.20.0) (2026-11-07)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions
</details>

<details><summary>kobako-mruby: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.19.0...kobako-mruby-v0.20.0) (2026-11-07)


### Miscellaneous Chores

* **kobako-mruby:** Synchronize kobako crates versions
</details>

<details><summary>kobako-io: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.19.0...kobako-io-v0.20.0) (2026-11-07)


###   BREAKING CHANGES

* **io:** a path holding a NUL byte now raises ArgumentError from every File and Dir call, including the exist?, file?, and directory? questions that answered false before.

### Features

* **io:** add File::Stat and the calls built on one ([c7174e6](https://github.com/elct9620/kobako/commit/c7174e6d43ffc9aa6503d237d7e08a3e4e0df9ab))
* **io:** list, make, and remove directories through Dir ([1b862a4](https://github.com/elct9620/kobako/commit/1b862a4b9d0d7d16a287585bef8d2615d4457242))
* **io:** name EAGAIN in the guest ([2ba8d24](https://github.com/elct9620/kobako/commit/2ba8d249dda1bbec2f4ac68b708e87116a07d3b1))
* **io:** name ENODEV in the guest ([6f00e26](https://github.com/elct9620/kobako/commit/6f00e262e2f5a8d823805cf165f0d0420144df7e))
* **io:** sync, truncate, and read or write a file at an offset ([7f61b7b](https://github.com/elct9620/kobako/commit/7f61b7b26fef46e1cb5996e04a5c1d1c6ae81536))


### Bug Fixes

* **io:** answer Dir, NUL paths, pread, and advise as CRuby does ([fd9accd](https://github.com/elct9620/kobako/commit/fd9accd485d3aad2f0f36a20a292d27b81f4777b))
</details>

<details><summary>kobako-regexp: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.19.0...kobako-regexp-v0.20.0) (2026-11-07)


###   BREAKING CHANGES

* **regexp:** refuse a braced hex escape and the absence operator
* **regexp:** read \w, \d, \s, \b and POSIX brackets by the (?a), (?d) and (?u) options
* **regexp:** These patterns now raise RegexpError where they used to answer. Remove the backreference, look-ahead or atomic group from a look-behind, and use `(?:...)` for a group inside a negative look-behind. Replace `(?s)` with `(?m)`, the language's spelling of the same option, and drop `(?U)`, `(?R)` and any other option outside `(?imxdau-imx)`. Rename a group whose name opens with a digit, and order an interval's bounds as `{lower,upper}`.
* **regexp:** `/(?<n>a)(b)/` no longer captures `(b)`, so `to_a`, `size`, `captures` and `$1` change; name the group to keep it. `\1`, `\k<1>`, `\g<1>` and `(?(1)...)` beside a named group now raise RegexpError; refer by name instead. `(?P<n>...)`, `(?P=n)` and `(?P>n)` now raise RegexpError; write `(?<n>...)`, `\k<n>` and `\g<n>`.

### Bug Fixes

* **regexp:** answer a condition that names a group ([faedca6](https://github.com/elct9620/kobako/commit/faedca617db4c859cd8bc9fcab3a599c6a188b22))
* **regexp:** carry capture names into a substitution block's match ([e3230b7](https://github.com/elct9620/kobako/commit/e3230b7213c2a70e14396408526a85ffa487621b))
* **regexp:** fold a quantifier on a group holding one quantified unit ([09ff373](https://github.com/elct9620/kobako/commit/09ff37345d31d81fb27fc59c5db5213aea0bc00a))
* **regexp:** fold case-insensitive classes as CRuby folds them ([ad1b211](https://github.com/elct9620/kobako/commit/ad1b211afd8bad77247bd9428d0242abe6cbae03))
* **regexp:** fold stacked quantifiers and repeat a look-around as CRuby does ([0ea692d](https://github.com/elct9620/kobako/commit/0ea692d1dce5622b90dc54d55c4a64efb90ff38d))
* **regexp:** leave the match globals as MRI does after finding every match ([3d3a7f9](https://github.com/elct9620/kobako/commit/3d3a7f9673f15fd63cb810dc4bf564388fa8e24b))
* **regexp:** match case-insensitive text across a multi-character fold ([0cb10e1](https://github.com/elct9620/kobako/commit/0cb10e1a363ab51a31198af9363e9493d77991e2))
* **regexp:** read \p{Graph}, \p{Print} and the properties the engine lacks as CRuby does ([8784d73](https://github.com/elct9620/kobako/commit/8784d73184f090cc915a97d4dcef65ef0d98efe5))
* **regexp:** read \w, \d, \s, \b and POSIX brackets by the (?a), (?d) and (?u) options ([655b5e5](https://github.com/elct9620/kobako/commit/655b5e5a6125726b91ceaaf540394d429a38709d))
* **regexp:** read a backreference level when the pattern does not recurse ([7ac2b7b](https://github.com/elct9620/kobako/commit/7ac2b7b648576e5160141436b905bac29945479f))
* **regexp:** read a pattern's groups as MRI does beside a named group ([15f6742](https://github.com/elct9620/kobako/commit/15f6742a141c506ef013623f79062a3d8a84964f))
* **regexp:** read a shared capture name as MRI does ([e855a5c](https://github.com/elct9620/kobako/commit/e855a5cc6588773e5852bc1b8d7230b569352dec))
* **regexp:** read inline options as MRI does ([191515b](https://github.com/elct9620/kobako/commit/191515b9cb49170156e4c78fac66c710a16e315b))
* **regexp:** read intervals as MRI does ([6eb853e](https://github.com/elct9620/kobako/commit/6eb853e6bfd69fd4a30f6f43f0df9c4f45965756))
* **regexp:** read octal, control and meaningless letter escapes as CRuby does ([e7b4355](https://github.com/elct9620/kobako/commit/e7b4355ad0bd597c7dce85ea7a5bd5270a1663e8))
* **regexp:** refuse a braced hex escape and the absence operator ([0c29dd8](https://github.com/elct9620/kobako/commit/0c29dd8358845f733a0de9ef5d816749655e857e))
* **regexp:** refuse a word boundary without the Unicode build ([d666e1f](https://github.com/elct9620/kobako/commit/d666e1fd54d890ffea61d9e78ae6723f7ceacd07))
* **regexp:** refuse the syntax MRI refuses ([da26615](https://github.com/elct9620/kobako/commit/da26615b7aa8378767574a7b579aea1d79b75bd9))
* **regexp:** resolve a backreference by a shared name as MRI does ([90a7100](https://github.com/elct9620/kobako/commit/90a7100fcba1122c5f132658df68a709ecec1755))
</details>

<details><summary>kobako-json: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.19.0...kobako-json-v0.20.0) (2026-11-07)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions
</details>

<details><summary>kobako-baker: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.19.0...kobako-baker-v0.20.0) (2026-11-07)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions
</details>

<details><summary>kobako-transport: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.19.0...kobako-transport-v0.20.0) (2026-11-07)


### Bug Fixes

* **transport:** call the Envelope by its name when one fails to decode ([411c8d5](https://github.com/elct9620/kobako/commit/411c8d505e2d939b8014b5406b1a9625b865f02b))
</details>

<details><summary>kobako-codec: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.19.0...kobako-codec-v0.20.0) (2026-11-07)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions
</details>

<details><summary>kobako-runtime: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.19.0...kobako-runtime-v0.20.0) (2026-11-07)


###   BREAKING CHANGES

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
</details>

<details><summary>kobako-wasmtime: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.19.0...kobako-wasmtime-v0.20.0) (2026-11-07)


###   BREAKING CHANGES

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
</details>

<details><summary>kobako-sdk: 0.20.0</summary>

## [0.20.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.19.0...kobako-sdk-v0.20.0) (2026-11-07)


###   BREAKING CHANGES

* **runtime:** spell each file type as Linux's FT_* names it
* **runtime:** name what a path reports Kstat, as Linux's struct kstat
* **runtime:** build an open's flags once and hand them to do_file_open
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
* **runtime:** build an open's flags once and hand them to do_file_open ([6672734](https://github.com/elct9620/kobako/commit/6672734fc42af8c2040402c8ed2b14d562c31328))
* **runtime:** let a Policy only refuse more than a Mount's access allows ([f1502b5](https://github.com/elct9620/kobako/commit/f1502b522bcb9d8fc8fffd29a7714b5d777bb793))
* **runtime:** list a directory as its Filesystem says, through dir_emit ([c7447a1](https://github.com/elct9620/kobako/commit/c7447a137a5237b2e071ba6500fddd736abe5092))
* **runtime:** name a listing's dots as Linux's drivers do ([ef26d61](https://github.com/elct9620/kobako/commit/ef26d61318ad3f489f5d1379db690854d46a2900))
* **runtime:** name in filesystem every type a Filesystem's operations take ([0cd4a9d](https://github.com/elct9620/kobako/commit/0cd4a9d9c90fd24ad40bd58448c57c8444111e30))


### Code Refactoring

* **runtime:** hang each entry's operations on its inode, as Linux does ([e21470b](https://github.com/elct9620/kobako/commit/e21470b625255eb85b4b5ff868872f2db5a7967e))
* **runtime:** land each read and write through a Kiocb, as read_iter and write_iter ([bc35dfd](https://github.com/elct9620/kobako/commit/bc35dfd13085b6aee517748996647bd7083cdbc8))
* **runtime:** let the mount layer own what a Guest may do beneath a Mount ([0dcde21](https://github.com/elct9620/kobako/commit/0dcde2143d7935a8e4b205bb03d664974af7edfb))
* **runtime:** name what a path reports Kstat, as Linux's struct kstat ([0f43bed](https://github.com/elct9620/kobako/commit/0f43bed4242c2236e02c88d00470d1d31180219e))
* **runtime:** name what an open file answers as Linux's file_operations ([ca835e9](https://github.com/elct9620/kobako/commit/ca835e99b821845bcfa56648ff7336c67242bbd7))
* **runtime:** spell each file type as Linux's FT_* names it ([796fac4](https://github.com/elct9620/kobako/commit/796fac412cae9c70b6f80e7f33946fe960ed520e))
</details>

---
This PR was generated with [Release Please](https://github.com/googleapis/release-please). See [documentation](https://github.com/googleapis/release-please#release-please).
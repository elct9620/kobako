# frozen_string_literal: true

# What a Filesystem written in Ruby answers from +stat+, and nothing beyond:
# +directory?+, +size+, and +ino+.
FilesystemStat = Data.define(:directory, :size, :ino) do
  def directory? = directory
end

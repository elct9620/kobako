# frozen_string_literal: true

# mruby source fixtures for the #install E2E. A File idiom whose pure
# methods run in-guest and whose I/O dispatches to a host backend, plus a
# depended-on/dependent pair whose cross-Extension constant reference
# resolves only at guest call time. Illustrative only — kobako ships no
# concrete Extension.
module ExtensionFixtures
  FILE_SOURCE = <<~RUBY
    class File < Kobako::Member
      def self.join(*parts)
        parts.join("/")
      end

      def self.basename(path)
        path.split("/").last || ""
      end

      def self.open(path)
        buffer = Buffer.new(read(path))
        return buffer unless block_given?

        begin
          yield buffer
        ensure
          buffer.close
        end
      end

      class Buffer
        def initialize(content)
          @content = content
        end

        def read
          @content
        end

        def close
          nil
        end
      end
    end
  RUBY

  # A depended-on Extension defining a constant, and a dependent Extension
  # whose method body references it — the reference resolving only at guest
  # call time, after every installed snippet has replayed.
  ERRNO_SOURCE = <<~RUBY
    module Errno
      ENOENT = 2
    end
  RUBY

  DEPENDENT_SOURCE = <<~RUBY
    module Status
      def self.missing_code
        Errno::ENOENT
      end
    end
  RUBY
end

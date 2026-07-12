# frozen_string_literal: true

# A minimal Hash-backed filesystem backend for the #install E2E. It answers
# the read / write / exist? calls a native-style guest `File` idiom
# dispatches to the host, doubling as a worked example of an Extension
# backend. kobako ships no concrete Extension — this is a test fixture.
class InMemoryFileSystem
  def initialize
    @files = {}
  end

  def read(path)
    @files.fetch(path.to_s)
  end

  def write(path, data)
    bytes = data.to_s
    @files[path.to_s] = bytes
    bytes.bytesize
  end

  def exist?(path)
    @files.key?(path.to_s)
  end
end

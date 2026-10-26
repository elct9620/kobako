# frozen_string_literal: true

# Opens each seeded path beneath ROOT with each mode, then reads, writes,
# and looks at what is left, answering one line per cell. It runs as is
# on CRuby and in the guest, so the two answer the same question.
#
# ROOT holds, for each mode index i, a file "fI" holding "hello", a
# directory "dI" holding "keep", nothing at "mI", a file "iI" for IO.new
# to wrap, and a file "nI" for IO.new to wrap with no mode.

MODES = [
  ["r", "r"], ["r+", "r+"], ["w", "w"], ["w+", "w+"], ["a", "a"], ["a+", "a+"],
  ["wx", "wx"], ["w+x", "w+x"],
  ["RDONLY", File::RDONLY], ["WRONLY", File::WRONLY], ["RDWR", File::RDWR],
  ["CREAT", File::CREAT], ["CREAT|EXCL", File::CREAT | File::EXCL],
  ["RDONLY|TRUNC", File::RDONLY | File::TRUNC], ["RDONLY|EXCL", File::RDONLY | File::EXCL],
  ["WRONLY|TRUNC", File::WRONLY | File::TRUNC], ["WRONLY|APPEND", File::WRONLY | File::APPEND],
  ["WRONLY|APPEND|TRUNC", File::WRONLY | File::APPEND | File::TRUNC],
  ["WRONLY|CREAT|EXCL", File::WRONLY | File::CREAT | File::EXCL],
  ["RDWR|CREAT|TRUNC", File::RDWR | File::CREAT | File::TRUNC],
  ["RDWR|APPEND|CREAT", File::RDWR | File::APPEND | File::CREAT]
].freeze

def matrix_attempt
  yield
rescue StandardError => e
  e.class.to_s
end

def matrix_after(path)
  return "dir" if File.directory?(path)
  return "none" unless File.exist?(path)

  File.read(path).inspect
end

# Each IO is flushed before the File closes its descriptor, so what it
# wrote is in the file when the cell looks.
def matrix_cell(path, mode)
  read, write = File.open(path, mode) do |file|
    stream = block_given? ? yield(file) : file
    [matrix_attempt { stream.read.inspect }, matrix_attempt { stream.write("Z") && stream.flush && "ok" }]
  end
  "read=#{read} write=#{write} after=#{matrix_after(path)}"
rescue StandardError => e
  "open=#{e.class} after=#{matrix_after(path)}"
end

# CRuby would close the wrapped descriptor again when the IO is collected,
# so the IO lets it go; the guest's IO never closes what it wraps.
def matrix_io(descriptor, *mode)
  io = IO.new(descriptor, *mode)
  io.autoclose = false if io.respond_to?(:autoclose=)
  io
end

def matrix_wrap(path, mode)
  File.open(path) do |wrapped|
    matrix_attempt { matrix_io(wrapped.fileno, mode) && "built" }
  end
end

lines = []
MODES.each_with_index do |(label, mode), i|
  { "file" => "f", "dir" => "d", "missing" => "m" }.each do |state, prefix|
    lines << "#{label} #{state}: #{matrix_cell("#{ROOT}/#{prefix}#{i}", mode)}"
  end
  lines << "#{label} IO.new: #{matrix_wrap("#{ROOT}/i#{i}", mode)}"
  lines << "#{label} IO.new no mode: #{matrix_cell("#{ROOT}/n#{i}", mode) { |file| matrix_io(file.fileno) }}"
end
lines

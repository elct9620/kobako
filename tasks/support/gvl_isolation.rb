# frozen_string_literal: true

# Reader behind +gate:gvl:isolation+: the +magnus+ mentions in a crate
# manifest, comment lines excluded. The GVL-released span calls into the
# wasmtime driver; if that driver's manifest declared +magnus+ the span could
# reach a Ruby VALUE, so a non-empty result is a structural-safety violation.
module KobakoGvlIsolation
  module_function

  # The stripped manifest lines that name +magnus+ as a whole word, skipping
  # comments so a mention in prose does not read as a dependency.
  def magnus_mentions(manifest_text)
    manifest_text.each_line
                 .reject { |line| line.strip.start_with?("#") }
                 .grep(/\bmagnus\b/i)
                 .map(&:strip)
  end
end

# frozen_string_literal: true

# The keyword options a direct +Kobako::Runtime.from_path+ call takes, keyed
# as +Kobako::SandboxOptions+ names them. Every cap is off so a test states
# only the option it is about; an override skips +SandboxOptions+ validation,
# which is how the ext's own fail-closed guards are reached.
module RuntimeOptions
  UNCAPPED = Kobako::SandboxOptions.new(timeout: nil, memory_limit: nil, stdout_limit: nil,
                                        stderr_limit: nil).to_h.freeze

  def runtime_options(**overrides)
    UNCAPPED.merge(overrides)
  end
end

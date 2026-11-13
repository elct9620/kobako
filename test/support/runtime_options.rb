# frozen_string_literal: true

# The keyword options a direct +Kobako::Runtime.from_path+ call takes, keyed
# as +Kobako::SandboxOptions+ names them. Every cap but the setup deadline is
# off so a test states only the option it is about; an override skips
# +SandboxOptions+, so a test reaches what the runtime refuses itself.
module RuntimeOptions
  UNCAPPED = %i[timeout memory_limit stdout_limit stderr_limit open_file_limit open_connection_limit
                connection_limit].to_h { |cap| [cap, nil] }.freeze

  def runtime_options(**overrides)
    Kobako::Runtime.default_options.merge(UNCAPPED, overrides)
  end
end

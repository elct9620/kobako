# Wire-symmetric payload peers

The payload types both Codec implementations carry, each registered on both
sides so neither peer can drop or reshape its half alone. The round-trip fuzz
holds what the bytes are; this holds that each peer still answers for them.

## Includes

- `lib/kobako/payload/**/*.rb`
- `crates/kobako-codec/src/**/*.rs`

## `Kobako::Payload::Arguments#encode`

The host peer writes the positional and keyword arguments a Call or a Run carries.

| Attribute | Value |
| --- | --- |
| internal | yes |

```ruby
module Kobako
  module Payload
    class Arguments
      def encode
      end
    end
  end
end
```

## `Kobako::Payload::Arguments.decode`

The host peer reads them back.

| Attribute | Value |
| --- | --- |
| internal | yes |

```ruby
module Kobako
  module Payload
    class Arguments
      def self.decode(bytes)
      end
    end
  end
end
```

## `Arguments::encode`

The guest-side peer writes the same arguments.

| Attribute | Value |
| --- | --- |
| internal | yes |

```rust
impl Encode for Arguments {
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {}
}
```

## `Arguments::decode`

The guest-side peer reads them back.

| Attribute | Value |
| --- | --- |
| internal | yes |

```rust
impl Decode for Arguments {
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {}
}
```

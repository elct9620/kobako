# frozen_string_literal: true

require "test_helper"

# Unit coverage of the value count a guest payload is held to before it
# decodes: every array, map, key, and element counts as one value, a str,
# bin, or ext body is one value however long, and bytes the count cannot
# frame are left for the decoder to refuse. The behaviour the count serves
# is witnessed where each decode site refuses.
class TestCodecValueBudget < Minitest::Test
  ValueBudget = Kobako::Codec::ValueBudget

  def count_refused?(bytes, limit)
    ValueBudget.assert_within!(bytes.b, limit)
    false
  rescue Kobako::Codec::InvalidTypeError
    true
  end

  def test_a_map_counts_its_keys_and_values
    # {1 => 2, 3 => 4}: the map and four entries, five values.
    map = "\x82\x01\x02\x03\x04"

    assert_equal [false, true], [count_refused?(map, 5), count_refused?(map, 4)],
                 "a map must count itself, each key, and each value"
  end

  def test_a_body_counts_once_however_long
    # ["a" * 20, ext 0x00 "s", bin of 3]: the array and three values.
    payload = "\x93\xb4#{"a" * 20}\xd4\x00s\xc4\x03xyz"

    assert_equal [false, true], [count_refused?(payload, 4), count_refused?(payload, 3)],
                 "a str, ext, or bin must count as one value whatever its length"
  end

  def test_a_payload_cut_short_counts_only_what_it_holds
    # An array of three whose last element never arrives: three values.
    cut = "\x93\x01\x02"

    assert_equal [false, true], [count_refused?(cut, 3), count_refused?(cut, 2)],
                 "a payload cut short must count the values it holds and leave the rest " \
                 "for the decoder to refuse"
  end

  def test_a_claimed_count_past_the_payload_is_left_for_the_decoder
    # array32 claiming four billion elements and holding none.
    header = "\xdd\xff\xff\xff\xff"

    refute count_refused?(header, 2),
           "a container claiming elements the payload never holds must be left for the decoder"
  end

  def test_no_budget_counts_nothing
    refute count_refused?("\x9f#{"\x00" * 15}", nil),
           "a payload must pass when the invocation has no memory cap"
  end
end

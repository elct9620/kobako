# Yield re-entry

What happens when a host Service calls back into the block the guest handed it.

## Includes

- `test/e2e/test_yield.rb`
- `test/e2e/test_yield_unwind.rb`
- `test/e2e/test_yield_block_failure.rb`
- `test/e2e/test_yield_block_spent.rb`
- `test/e2e/test_yield_value_refusal.rb`
- `test/unit/transport/test_yielder.rb`
- `test/parity/test_yield.rb`

### Why these scenarios

A yield turns one dispatch into a conversation: the guest calls out, the host calls back, and either side may end it. The scenarios follow every way that conversation can close — a value, a break, a fall-through, a raise — because each unwinds a different distance.

The block-failure scenarios are about what a failure leaves behind. A Service that rescues one raise, holds it, and yields again must not answer the second block with the first block's failure, and a failure already rescued must not reappear as a later refusal. Both are witnessed because neither shows up in the single-yield case.

A block's answer is restored on its way in and a break's value is not, which is the one asymmetry here. The exits that raise are followed too: an unwind aimed past the boundary, an answer the wire cannot carry, and a Yielder reached after its frame returned each end the conversation somewhere the ordinary closes cannot reach.

## `T-083` A Service can tell that the guest passed it a block

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that reports whether it was given a block |
| When | guest code calls it with a block |
| Then | the Service reports that it was |

## `T-084` And that it was not

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that reports whether it was given a block |
| When | guest code calls it without a block |
| Then | the Service reports that it was not |

## `T-085` Yielding answers with what the block said

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields once |
| When | guest code calls it with a block returning a value |
| Then | the Service receives that value from the yield |

## `T-086` Yielding several times runs the block each time

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields several times |
| When | guest code calls it with a block |
| Then | the block runs once per yield |

## `T-087` A block may reach back out to another Service

| Step | Statement |
| --- | --- |
| Given | a Sandbox with two bound Services, one of which yields |
| When | guest code's block calls the other Service |
| Then | the nested call answers and the outer yield receives the block's value |

## `T-088` A Service given a block need not use it

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that never yields |
| When | guest code calls it with a block |
| Then | the call answers and the block never runs |

## `T-089` Breaking out of a block unwinds the Service

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields |
| When | guest code's block breaks with a value |
| Then | the call answers that value without the Service finishing |

## `T-090` Breaking out of a lambda leaves the Service running

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields to a lambda |
| When | the lambda breaks with a value |
| Then | the yield receives that value and the Service continues |

## `T-091` A break value the wire cannot carry is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields |
| When | guest code's block breaks with a value having no wire representation |
| Then | the invocation fails rather than carrying it across |

## `T-092` Each nested frame carries its own block

| Step | Statement |
| --- | --- |
| Given | a Sandbox with bound Services yielding into one another |
| When | guest code drives two nested yields |
| Then | each frame yields to the block it was given |

## `T-093` A refused nested call leaves the outer block usable

| Step | Statement |
| --- | --- |
| Given | a Sandbox with bound Services yielding into one another |
| When | the inner call is refused and the outer block runs on |
| Then | the outer yield still answers |

## `T-094` A raise inside the block surfaces where the Service yielded

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields |
| When | guest code's block raises |
| Then | the Service sees the failure at its yield |

## `T-095` An unrescued block raise stays the guest's own exception

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields |
| When | guest code's block raises and nobody rescues it |
| Then | the guest can rescue it as the exception it raised |

## `T-096` A Service that rescues the block reports its own failure instead

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that rescues what its yield raised |
| When | guest code's block raises |
| Then | the guest sees the Service's failure rather than its own |

## `T-097` A rescued block failure leaves nothing behind

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service rescued a raise from the block it yielded to |
| When | the invocation continues |
| Then | no trace of that failure reaches the next yield |

## `T-098` A held failure answers only for the block that raised it

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service holds a failure from one block |
| When | another block is yielded to |
| Then | the held failure does not answer for it |

## `T-099` A rescued failure does not answer that block's later refusal

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service rescued one failure from a block |
| When | the same block is refused later for another reason |
| Then | the later refusal is reported as its own |

## `T-100` A reference in the block's answer becomes its object

| Step | Statement |
| --- | --- |
| Given | a yield whose block answered with a capability reference |
| When | the Service receives the answer |
| Then | it holds the original host object |

## `T-101` An answer carrying no reference crosses unchanged

| Step | Statement |
| --- | --- |
| Given | a yield whose block answered with an ordinary value |
| When | the Service receives the answer |
| Then | it holds that value unchanged |

## `T-102` A break value is not restored on its way out

| Step | Statement |
| --- | --- |
| Given | a yield whose block broke with a capability reference |
| When | the break unwinds |
| Then | the reference passes through without being restored |

## `T-103` Both frontends yield and answer the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose Service yields once to a guest block |
| When | both frontends run it |
| Then | they observe the same value |

## `T-104` Both unwind a break and fall through a next the same way

| Step | Statement |
| --- | --- |
| Given | a scenario exercising both block exits |
| When | both frontends run it |
| Then | they observe the same values |

## `T-105` Both carry a nested dispatch through a block the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose block dispatches to another Service |
| When | both frontends run it |
| Then | they observe the same value |

## `T-106` Both refuse a block exit aimed past the boundary the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose block tries to leave past the yield boundary |
| When | both frontends run it |
| Then | they refuse it the same way |

## `T-107` Both surface an unrescued block raise the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose block raises with nobody to rescue it |
| When | both frontends run it |
| Then | they attribute it the same way |

## `T-134` An unwind aimed past the yield boundary

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields |
| When | guest code's block returns to an enclosing method still on the guest stack |
| Then | the invocation fails naming a local jump |

## `T-135` An answer the wire cannot carry is refused at the yield site

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields |
| When | guest code's block answers a value having no wire representation |
| Then | the invocation fails rather than carrying a coerced value across |

## `T-136` A Yielder reached after its frame returned

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that stored the block it was yielded |
| When | a later dispatch calls that stored block |
| Then | the invocation fails naming a local jump |

## `T-154` A yield argument the host cannot write is the Service's to handle

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding a value having no wire representation |
| Given | the Service rescuing that refusal |
| When | guest code calls it with a block |
| Then | the invocation answers what the Service returned |

## `T-155` The refusal names the position it failed at

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding a value having no wire representation |
| When | the Service rescues the refusal and reads it |
| Then | it names the yield the value could not cross |

## `T-156` The block never runs

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding a value having no wire representation |
| When | guest code's block records that it ran |
| Then | the record shows it did not |

## `T-157` Unrescued, it is the Service that failed

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding a value having no wire representation |
| When | the Service leaves the refusal unrescued |
| Then | `Kobako::ServiceError` reaches the Host App |

## `T-158` The refusal is worded as kobako's own

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service left a yield refusal unrescued |
| When | the Host App reads the failure's message |
| Then | it does not wear the shape a Service's own exception crosses in |

## `T-159` A yield argument that nests without end refuses at the same site

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding a value that nests without end |
| Given | the Service rescuing that refusal |
| When | guest code calls it with a block |
| Then | the invocation answers what the Service returned |

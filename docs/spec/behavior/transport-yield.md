# Yield re-entry

What happens when a host Service calls back into the block the guest handed it.

### Why these scenarios

A yield turns one dispatch into a conversation: the guest calls out, the host calls back, and either side may end it. The scenarios follow every way that conversation can close — a value, a break, a fall-through, a raise — because each unwinds a different distance.

The block-failure scenarios are about what a failure leaves behind. A Service that rescues one raise, holds it, and yields again must not answer the second block with the first block's failure, and a failure already rescued must not reappear as a later refusal. Both are witnessed because neither shows up in the single-yield case.

A block's answer is restored on its way in and a break's value is not, which is the one asymmetry here. The exits that raise are followed too: an unwind aimed past the boundary, an answer the wire cannot carry, and a Yielder reached after its frame returned each end the conversation somewhere the ordinary closes cannot reach. That last exit has no parity scenario, because the frontends close it at different times: one refuses the stored block when it runs, the other lends its Yielder for the frame alone so a Service storing it never builds. Each is witnessed on its own frontend.

## Includes

- `test/e2e/test_yield*.rb`
- `test/unit/transport/test_yielder.rb`
- `test/parity/test_yield.rb`
- `crates/kobako/src/dispatch.rs`
- `crates/kobako/src/yielder.rs`
- `crates/kobako/src/msgpack/yielder.rs`
- `crates/kobako/tests/byte_surface.rs`

## `T-083` A Service can tell that the guest passed it a block

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that reports whether it was given a block |
| When | guest code calls it with a block |
| Then | the Service reports that it was |

## `T-084` A Service can tell that the guest passed it no block

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
| Then | the outer yield receives the block's value, built from the nested call's answer |

## `T-088` A Service given a block need not use it

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that never yields |
| When | guest code calls it with a block |
| Then | the call answers without running the block |

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
| Then | the Service goes on to answer with that value |

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

## `T-104` Both frontends unwind a break and fall through a next the same way

| Step | Statement |
| --- | --- |
| Given | a scenario exercising both block exits |
| When | both frontends run it |
| Then | they observe the same values |

## `T-105` Both frontends carry a nested dispatch through a block the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose block dispatches to another Service |
| When | both frontends run it |
| Then | they observe the same value |

## `T-106` Both frontends refuse a block exit aimed past the boundary the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose block tries to leave past the yield boundary |
| When | both frontends run it |
| Then | they refuse it the same way |

## `T-107` Both frontends surface an unrescued block raise the same way

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
| Then | it reaches the Host App as a Service failure |

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

## `T-165` A guest that trapped inside a block aborts the yield

| Step | Statement |
| --- | --- |
| Given | a Service yielding to a block |
| When | the guest traps before answering |
| Then | the yield is abandoned rather than answered |

## `T-166` A yield answer the envelope cannot frame aborts the yield

| Step | Statement |
| --- | --- |
| Given | a Service yielding to a block |
| When | the answer is bytes the envelope cannot frame |
| Then | the yield is abandoned rather than answered |

## `T-170` A yield on the byte seam carries the Host App's own bytes

| Step | Statement |
| --- | --- |
| Given | a Service yielding arguments the Host App encoded itself |
| When | the block answers |
| Then | the block's own answer comes back as bytes for the Host App to read |

## `T-172` A yield through the schema seam ships one frame and reads the answer back

| Step | Statement |
| --- | --- |
| Given | a Service yielding values rather than bytes |
| When | the block answers |
| Then | the arguments crossed as one frame |

## `T-184` A yield failure is categorised by whose failure it is

| Step | Statement |
| --- | --- |
| Given | each way a yield can end badly |
| When | the failure is categorised |
| Then | each names the side it belongs to, carrying what a guest needs to continue |

## `T-187` An unrescued block raise reaches the Host App as the guest's own failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields and lets its block's failure through |
| When | guest code's block raises and nothing in the guest rescues it |
| Then | it fails as a Sandbox failure under the class the guest raised |

## `T-188` A break from a nested block ends only the Service that yielded to it

| Step | Statement |
| --- | --- |
| Given | a Sandbox with bound Services yielding into one another |
| When | the inner block breaks with a value |
| Then | the outer block resumes with that value as the inner call's answer |

## `T-189` A Service's answer is its own, not its block's

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields several times and answers a value of its own |
| When | guest code calls it with a block |
| Then | the call answers the Service's value rather than the block's last one |

## `T-202` A Service's own failure crosses under its class name

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that rescues what its yield raised and raises its own exception |
| When | guest code's block raises |
| Then | the failure's message leads with the Service exception's class |

## `T-203` A block exit the wire cannot carry reaches the Host App as a Service failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields |
| Given | a guest block that returns past the boundary, answers a value, or breaks with a value having no wire representation |
| When | nobody rescues the failure |
| Then | it fails as a Service failure |

## `T-204` A refused block value names the type error it stands for

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that yields |
| When | guest code's block breaks with a value having no wire representation |
| Then | the failure's message names the type error and the slot it stopped at |

## `T-205` A Yielder reached after its frame returned is the Service's failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that stored the block it was yielded |
| When | a later dispatch calls that stored block and nobody rescues the failure |
| Then | it fails as a Service failure |

## `T-216` A yield whose arguments nest past the wire's depth refuses at the yield site

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding arguments nested one level past the deepest the wire encodes |
| Given | the Service rescuing that refusal |
| When | guest code calls it with a block |
| Then | the invocation answers what the Service returned |

## `T-217` Yield arguments at the wire's depth reach the block

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding arguments nested to the deepest level the wire encodes, counted as the one list they travel in |
| When | guest code calls it with a block that measures what it received |
| Then | the block receives them nested to that depth |

## `T-231` A block takes a yield as a block takes arguments

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding whatever it was called with |
| When | its block declares fewer arguments than it is yielded, or more |
| Then | the extras are dropped and the missing ones are nil |

## `T-232` A lambda block refuses a yield of the wrong count

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding two arguments |
| When | guest code passes it a lambda taking one as the block |
| Then | the Service's yield fails with the guest's `ArgumentError` |

## `T-233` `next` answers the yield as falling through does

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding twice |
| When | the block ends once with `next` and a value and once by falling through |
| Then | each yield receives the value its block ended with |

## `T-248` Time around a yield counts against the deadline

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service spends past the deadline before yielding |
| When | guest code calls it with a block that does nothing |
| Then | the invocation ends at the deadline |

## `T-249` Memory a yielded block grows counts against the budget

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service yields to a block that grows memory |
| When | the budget is too small for what the block grows |
| Then | the invocation fails as the budget's own trap |

## `T-250` An unframeable yield answer fails the Service's yield as a trap

| Step | Statement |
| --- | --- |
| Given | a Service yielding to a block |
| When | the guest answers with no bytes, or with an arm outside the live set |
| Then | the Service's yield fails as a trap |

## `T-253` The block reaches the Service as an ordinary block

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service that hands its block on to code knowing nothing of kobako |
| When | guest code calls it with a block |
| Then | that code runs the guest block as it would any other |

## `T-255` A frontend lending its Yielder for the frame alone cannot store it

| Step | Statement |
| --- | --- |
| Given | a frontend whose Yielder borrows the dispatch frame |
| When | a Service is written to keep that Yielder for a later dispatch |
| Then | the Service does not build |

## `T-262` Nested yields carry no depth limit of their own

| Attribute | Value |
| --- | --- |
| unverifiable | no depth is the last, so a finite test cannot show there is no limit |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service that yields to a block calling the Service again |
| When | guest code nests those yields ever deeper |
| Then | only the guest's stack bounds the depth |

## `T-264` A guest that trapped is not entered again

| Step | Statement |
| --- | --- |
| Given | a Service that rescues the failure of a yield whose block trapped |
| When | the Service yields to the block again |
| Then | the block does not run again |

## `T-268` A yield through the schema seam reads the block's answer back as a value

| Step | Statement |
| --- | --- |
| Given | a Service yielding values rather than bytes |
| When | the block answers |
| Then | the Service receives the answer as this schema's value |

## `T-276` A Service rescuing a trapped yield does not hide the trap

| Step | Statement |
| --- | --- |
| Given | a Service that rescues the failure of a yield whose block trapped |
| When | the Service yields to the block again |
| Then | the invocation fails as the first trap |

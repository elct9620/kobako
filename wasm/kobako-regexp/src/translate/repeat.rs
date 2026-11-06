//! How MRI reads quantifiers stacked on one unit. Onigmo folds a quantifier
//! over another into one, or into a fixed pair, before matching; the engine
//! nests them as written, which can answer differently.

/// One quantifier: how often it repeats, and how. `upper` is `None` without a
/// bound.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Quantity {
    pub(super) lower: u64,
    pub(super) upper: Option<u64>,
    pub(super) greedy: bool,
    pub(super) possessive: bool,
}

impl Quantity {
    pub(super) const fn greedy(lower: u64, upper: Option<u64>) -> Self {
        Self {
            lower,
            upper,
            greedy: true,
            possessive: false,
        }
    }

    const fn lazy(lower: u64, upper: Option<u64>) -> Self {
        Self {
            greedy: false,
            ..Self::greedy(lower, upper)
        }
    }

    /// `?`, `*`, `+`, `??`, `*?` or `+?` as Onigmo numbers them; any other
    /// quantifier folds with nothing.
    fn popular(self) -> Option<usize> {
        let base = match (self.lower, self.upper) {
            (0, Some(1)) => 0,
            (0, None) => 1,
            (1, None) => 2,
            _ => return None,
        };
        Some(if self.greedy { base } else { base + 3 })
    }

    fn write(self, out: &mut String) {
        match (self.lower, self.upper) {
            (0, Some(1)) => out.push('?'),
            (0, None) => out.push('*'),
            (1, None) => out.push('+'),
            (lower, None) => out.push_str(&format!("{{{lower},}}")),
            (lower, Some(upper)) if lower == upper => out.push_str(&format!("{{{lower}}}")),
            (lower, Some(upper)) => out.push_str(&format!("{{{lower},{upper}}}")),
        }
        if !self.greedy {
            out.push('?');
        }
    }
}

#[derive(Clone, Copy)]
enum Fold {
    AsIs,
    /// The outer quantifier adds nothing.
    Inner,
    Star,
    LazyStar,
    LazyOptional,
    /// `(?:x+)??`
    LazyOptionalPlus,
}

/// CRuby's copy of Onigmo's table, by the inner quantifier's row and the
/// outer one's column.
const FOLDS: [[Fold; 6]; 6] = {
    use Fold::*;
    [
        [Inner, Star, Star, LazyOptional, LazyStar, AsIs],
        [
            Inner,
            Inner,
            Inner,
            LazyOptionalPlus,
            LazyOptionalPlus,
            Inner,
        ],
        [Star, Star, Inner, AsIs, LazyOptionalPlus, Inner],
        [Inner, LazyStar, LazyStar, Inner, LazyStar, LazyStar],
        [Inner, Inner, Inner, Inner, Inner, Inner],
        [AsIs, AsIs, AsIs, LazyStar, LazyStar, Inner],
    ]
};

/// The engine's text for `unit` repeated by each quantifier in turn.
///
/// A possessive quantifier is a quantifier MRI folds like any other, then
/// makes atomic, so the stack under it folds first and the atomic group is
/// a new unit.
pub(super) fn write(unit: &str, quantities: &[Quantity]) -> String {
    let mut text = unit.to_string();
    let mut current: Option<Quantity> = None;
    for &quantity in quantities {
        let outer = Quantity {
            possessive: false,
            ..quantity
        };
        current = Some(match current {
            None => outer,
            Some(inner) => fold(&mut text, inner, outer),
        });
        if quantity.possessive {
            if let Some(folded) = current.take() {
                folded.write(&mut text);
            }
            text = format!("(?>{text})");
        }
    }
    if let Some(quantity) = current {
        quantity.write(&mut text);
    }
    text
}

/// Folds `outer` over `text` already repeated by `inner`, leaving in `text`
/// the unit the answer repeats.
fn fold(text: &mut String, inner: Quantity, outer: Quantity) -> Quantity {
    let outer = bound_by_inner_star(inner, outer);
    let folded = match (inner.popular(), outer.popular()) {
        (Some(row), Some(column)) => FOLDS[row][column],
        _ => Fold::AsIs,
    };
    match folded {
        Fold::Inner => inner,
        Fold::Star => Quantity::greedy(0, None),
        Fold::LazyStar => Quantity::lazy(0, None),
        Fold::LazyOptional => Quantity::lazy(0, Some(1)),
        Fold::LazyOptionalPlus => {
            *text = format!("(?:{text}+)");
            Quantity::lazy(0, Some(1))
        }
        Fold::AsIs => {
            inner.write(text);
            *text = format!("(?:{text})");
            outer
        }
    }
}

/// A greedy bounded interval over `*` or `+` repeats it no more than its
/// lower bound, or once, as Onigmo shortens it.
fn bound_by_inner_star(inner: Quantity, outer: Quantity) -> Quantity {
    let inner_star = matches!(inner.popular(), Some(1 | 2));
    match outer.upper {
        Some(upper) if inner_star && outer.greedy && upper > 1 => Quantity {
            upper: Some(outer.lower.max(1)),
            ..outer
        },
        _ => outer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAR: Quantity = Quantity::greedy(0, None);
    const OPTIONAL: Quantity = Quantity::greedy(0, Some(1));
    const LAZY_PLUS: Quantity = Quantity::lazy(1, None);

    #[test]
    fn folds_a_quantifier_over_another_as_onigmo_does() {
        assert_eq!(write("a", &[STAR, STAR]), "a*");
        assert_eq!(write("a", &[LAZY_PLUS, OPTIONAL]), "(?:a+?)?");
        assert_eq!(write("a", &[LAZY_PLUS, OPTIONAL, STAR]), "(?:a+?)*");
        assert_eq!(write("a", &[LAZY_PLUS, STAR]), "(?:a+?)*");
        assert_eq!(write("a", &[STAR, Quantity::lazy(0, None)]), "(?:a+)??");
    }

    #[test]
    fn makes_a_possessive_quantifier_atomic_after_folding() {
        let possessive_plus = Quantity {
            possessive: true,
            ..Quantity::greedy(1, None)
        };
        assert_eq!(write("a", &[STAR, possessive_plus]), "(?>a*)");
        assert_eq!(write("a", &[possessive_plus, STAR]), "(?>a+)*");
    }

    #[test]
    fn shortens_a_bounded_interval_over_a_star() {
        assert_eq!(
            write("a", &[STAR, Quantity::greedy(2, Some(3))]),
            "(?:a*){2}"
        );
        assert_eq!(write("a", &[STAR, Quantity::greedy(0, Some(3))]), "a*");
    }
}

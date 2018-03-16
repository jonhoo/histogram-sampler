extern crate rand;

use std::collections::BTreeMap;
use std::collections::Bound;

pub struct Sampler {
    bins: BTreeMap<usize, usize>,
    end: usize,
}

impl Sampler {
    pub fn from_bins<I>(iter: I, bin_width: usize) -> Self
    where
        I: IntoIterator<Item = (usize, usize)>,
    {
        let mut start = 0;
        let mut next_id = 0;
        let mut bins = BTreeMap::default();
        let first_bin_avg = bin_width / 4;
        let first_bin_avg_offby = bin_width % 4;

        for (bin, count) in iter {
            // we want the likelihood of selecting an id in this bin to be proportional to
            // average bin value * `count`. the way to think about that in the context of sampling
            // from a histogram is that there are `count` ranges, each spanning an interval of
            // width `bin`. we'll keep track of where the last such interval ended (`start`-1), and
            // start each new interval where the last one ended to get a single large interval
            // covering all "values" that we can sample uniformly from.
            for id in 0..count {
                bins.insert(start, next_id + id);

                // the bucket *centers* on bin, so it captures everything within bin_width/2 on
                // either side. in general, we should therefore have start entries for it. the
                // exception is the very first bin, which only holds things in [0, bin_width/2),
                // since everything above that would be rounded to the *next* bucket. so, for
                // things in the very first bin, we *actually* want to generate bin_width/4 items
                // to select from.
                if bin == 0 {
                    // rounding is a little awkward here. we want all the things in the first
                    // bin to *average* to bin_width / 4. first_bin_avg is rounded down with
                    // some remainder. we want to make sure to distribute in that remainder
                    if first_bin_avg_offby == 0 {
                        // no remainder to worry about
                        start += first_bin_avg;
                    } else if first_bin_avg_offby == 2 {
                        // first_bin_avg is off by 0.5 so add 1 for 1/2 of the things
                        start += first_bin_avg + (id % 2);
                    } else {
                        // avg is off by 0.25 or 0.75 so add 1 for 1/4 or 3/4 of the things
                        let one_every_four = if id % 4 == 0 { 1 } else { 0 };
                        if first_bin_avg_offby == 1 {
                            start += first_bin_avg + one_every_four;
                        } else {
                            start += first_bin_avg + 1 - one_every_four;
                        }
                    }
                } else {
                    start += bin;
                }
            }
            next_id += count;
        }

        Sampler {
            bins: bins,
            end: start,
        }
    }
}

impl rand::distributions::Sample<usize> for Sampler {
    fn sample<R: rand::Rng>(&mut self, rng: &mut R) -> usize {
        use rand::distributions::IndependentSample;
        self.ind_sample(rng)
    }
}

impl rand::distributions::IndependentSample<usize> for Sampler {
    fn ind_sample<R: rand::Rng>(&self, rng: &mut R) -> usize {
        *self.bins
            .range((
                Bound::Unbounded,
                Bound::Included(rng.gen_range(0, self.end)),
            ))
            .next_back()
            .unwrap()
            .1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let data = vec![
            (0, 16724),
            (10, 16393),
            (20, 4601),
            (30, 1707),
            (40, 680),
            (50, 281),
            (60, 128),
            (70, 60),
            (80, 35),
            (90, 16),
            (100, 4),
            (110, 4),
            (120, 10),
            (130, 1),
            (140, 2),
            (160, 1),
            (210, 1),
            (250, 1),
            (290, 1),
        ];

        let ndata: isize = data.iter().map(|&(_, n)| n as isize).sum();
        let nvotes = data.iter()
            .map(|&(bin, n)| (bin * n) as isize)
            .sum::<isize>()
            + (data.iter().next().unwrap().1 as f64 * 0.25) as isize;
        let proportions: Vec<_> = data.iter()
            .map(|&(bin, n)| (bin, (n as f64 / ndata as f64)))
            .collect();

        let votes_per_story = Sampler::from_bins(data, 10);

        use std::ops::AddAssign;
        use std::collections::HashMap;
        use rand::distributions::IndependentSample;

        let mut rng = rand::thread_rng();
        let mut votes = HashMap::new();
        for _ in 0..nvotes {
            votes
                .entry(votes_per_story.ind_sample(&mut rng))
                .or_insert(0)
                .add_assign(1);
        }
        let nstories = votes.len() as isize;

        // number of stories should be roughly (< 5%) the same
        println!("story count: {} vs {}", nstories, ndata);
        assert!((nstories - ndata).abs() < ndata / 20);

        let mut hist = HashMap::new();
        for (_, votes) in votes {
            hist.entry(10 * ((votes + 5) / 10))
                .or_insert(0)
                .add_assign(1);
        }

        let ourvotes = hist.iter().map(|(&bin, &n)| bin * n).sum::<isize>()
            + (hist[&0] as f64 * 0.25) as isize;
        // number of stories should be roughly (< 5%) the same
        println!("vote count: {} vs {}", nvotes, ourvotes);
        assert!((nvotes - ourvotes).abs() < nvotes / 20);

        let mut keys: Vec<_> = hist.keys().cloned().collect();
        keys.sort();

        let mut expected_props = proportions.iter().peekable();
        for bin in keys {
            let prop = hist[&bin] as f64 / nstories as f64;

            if let Some(&&(exp_bin, exp_prop)) = expected_props.peek() {
                let diff = prop - exp_prop;

                if exp_bin == bin as usize {
                    println!(
                        "{}\t{:.1}%\t->\t{}\t{:.1}%\t(diff: {:>5.2})",
                        exp_bin,
                        100.0 * exp_prop,
                        bin,
                        100.0 * prop,
                        100.0 * diff
                    );
                } else {
                    println!(
                        "{}\t{:.1}%\t??\t{}\t{:.1}%",
                        exp_bin,
                        100.0 * exp_prop,
                        bin,
                        100.0 * prop,
                    );
                }

                if exp_bin <= bin as usize {
                    expected_props.next();
                }

                if prop > 0.01 {
                    // any bucket with 1% or more stories shoud match pretty well (within 5pp)
                    assert!(diff.abs() < 5.0);
                }
            } else {
                println!("\t\t??\t{}\t{:.1}%", bin, 100.0 * prop);
                assert!(prop < 0.01);
            }
        }
    }
}

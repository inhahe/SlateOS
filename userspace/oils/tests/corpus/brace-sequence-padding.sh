# A numeric brace sequence is rendered with `printf`'s `%0*d`, so the width a
# zero-padded endpoint asks for is a count of *columns*, sign included — the
# negative values spend one on the `-` and the positive ones fill it with a
# zero. Being negative is not itself a request for padding, and `-0` is a
# signed zero rather than a padded number.
echo "=== a padded endpoint sets the width for both signs"
echo {-01..01}
echo {-001..1}
echo {01..-1}
echo {0..-05}
echo {-05..05..2}
echo {010..-010..5}
echo {-00..1}

echo "=== …and an unpadded one asks for nothing"
echo {-1..01}
echo {-2..2}
echo {-2..2..2}
echo {-10..10..5}
echo {-0..0}
echo {-0..-0}
echo {1..3}

echo "=== when both ask, the wider wins, in either position"
echo {01..0001}
echo {0001..01}
echo {1..0001}
echo {0001..1}
echo {-0001..-01}
echo {00..0}
echo {0..00}

echo "=== the plain padded cases are unchanged"
echo {01..10}
echo {001..10}
echo {1..010}
echo {09..11}
echo {0001..3}
echo {08..10}
echo {10..01}

echo "=== a step does not change the width"
echo {1..10..3}
echo {01..10..3}
echo {010..001..3}
echo {1..3..01}

echo "=== a sequence inside a word, and crossed with another"
echo x{-01..01}y
echo {-01..01}{a,b}
echo {01..02}{-01..01}

echo "=== done"

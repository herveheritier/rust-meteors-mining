#!/bin/sh
clear
ls source
~/Documents/qb64pe-v4.5/qb64pe -x ./source/$1.bas -o ./build/$1 && ./build/$1

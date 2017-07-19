/**
 * @brief TPP (Transport Plus Protocol)  header structure
 */
#ifndef TPP_H_
#define TPP_H_

#include "typedef.h"

/* The TPP Segment 
 0              15               31
|----------------|----------------|
| Soruce Port    | Dest. Port     |
| (th_sport)     | (th_dport)     |
|+++++++++++++++++++++++++++++++++|
| Segment Size                    |
| (th_sz_seg)                     |
|+++++++++++++++++++++++++++++++++|
| Sequence Number                 |
| (th_seq)                        |
|+++++++++++++++++++++++++++++++++|
| Acknowledgement  Number         |
| (th_ack)                        |
|+++++++++++++++++++++++++++++++++|
|S|A|F|unsused   | Checksum       |
|(th_flgs)|(rh_x)| (rh_checksum)  |
|----------------|----------------|
| Payload ... (not header!)       |
|+++++++++++++++++++++++++++++++++|
 
S: SNY 1 bit
A: ACK 1 bit
F: FIN 1 bit

*/


struct tpphdr {
	U16	th_sport;	// source port
	U16	th_dport;	// destination port
	U32	th_sz_seg;	// segment size
	U32	th_seq;		// sequence number
	U32	th_ack;		// ack number
	U8	th_flags;	// SYN, ACK, FIN
#define TH_SYN  0x80
#define TH_ACK	0x40
#define TH_FIN  0x20
	U8	th_x;		// unusued
	U16	th_checksum;	// checksum
};

#define TPPHDR_LEN sizeof(struct tpphdr)	// tpp header size in bytes

// host byte order assumed
extern void display_tpphdr(struct tpphdr *p);
extern void init_seg_hdr_h(struct tpphdr *p, U16 n_sport, U16 n_dport); 
extern void set_seg_size_h(struct tpphdr *p, U32 size);
extern void set_seg_seq_h(struct tpphdr *p, tpp_seq seq);
extern void set_seg_ack_h(struct tpphdr *p, tpp_seq ack);
extern void set_seg_flags_h(struct tpphdr *p, U8 flags);
extern void set_seg_checksum_h(struct tpphdr *p, U16 checksum);

// byte order conversion functions
extern void hton_seg(void *buf);
extern void ntoh_seg(void *buf);

#endif // TPP_H_

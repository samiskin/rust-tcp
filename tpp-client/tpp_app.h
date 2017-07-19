/**
 * @brief tpp application layer related header file
 */

#ifndef TPP_APP_H_
#define TPP_APP_H_

#include "stddef.h"
#include "typedef.h"

#define NUM_CLIENTS 2
#define MAX_BUF_LEN 65536

struct payload {
	size_t len;		// length of the bytes that follows
	char   *p_bytes;	// stream of bytes
};


//extern int  unpack_payload(void *buf, size_t len, struct payload *p);
//extern void pack_payload(void *buf, struct payload *p);
extern void display_payload(struct payload *p);


#endif // ! TPP_APP_H_
